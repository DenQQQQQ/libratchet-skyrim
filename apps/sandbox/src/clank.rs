//! Clank on Ratchet's back, when `riftapart-assets/clank.glb` exists (converted from the user's
//! own install by `tools/extract_riftapart_ratchet.py`).
//!
//! Like the game, Clank's `dyn_backpack_attach` joint is placed on Ratchet's `ClankAttach` joint
//! (both carry the `igLoc_clankBackpack` locator). He plays his backpack idle, and his broken
//! heli-pack extends while Ratchet glides and retracts afterwards. The heli-pack meshes are their
//! own model looks, shown only while it's out, and the rotors are spun in code like the game does.

use crate::Player;
use bevy::app::AnimationSystems;
use bevy::gltf::{Gltf, GltfExtras};
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use libratchet::MoveState;
use std::collections::HashMap;
use std::time::Duration;

pub const MODEL_FILE: &str = "clank.glb";
const ATTACH_JOINT: &str = "dyn_backpack_attach";
const CLIPS: &[&str] = &["idle", "glide", "heli_extend", "heli_retract"];
/// Rotor heads (top and both forearms); the game spins them in code, not in the clips.
const ROTORS: &[&str] = &["topProp", "LF_Prop", "RT_Prop"];
const ROTOR_SPEED: f32 = 22.0; // rad/s
/// Clips during which the heli-pack is out.
const HELI_CLIPS: &[&str] = &["heli_extend", "glide", "heli_retract"];

#[derive(Resource)]
struct ClankGltf(Handle<Gltf>);

#[derive(Resource, Default)]
struct ClankRig {
    /// Clank's scene root and his attach joint, re-snapped every frame (the backpack clips
    /// move the attach joint).
    root: Option<Entity>,
    attach: Option<Entity>,
    player: Option<Entity>,
    clips: HashMap<&'static str, AnimationNodeIndex>,
    current: Option<&'static str>,
    /// Seconds left of a one-shot clip (heli extend/retract) before moving on.
    one_shot: f32,
    gliding: bool,
    /// Rotor spin joints with their spin axis in the joint's local space.
    rotors: Vec<(Entity, Vec3)>,
    rotor_angle: f32,
    /// Heli-pack mesh nodes (glTF extras `"part": "heli"` from the converter) and whether
    /// they're shown.
    heli_parts: Vec<Entity>,
    heli_shown: bool,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ClankRig>()
        .add_systems(Update, (setup_clips, play_clips, show_heli, follow_attach).chain())
        .add_systems(PostUpdate, spin_rotors.after(AnimationSystems).before(TransformSystems::Propagate));
}

/// Spawns Clank under Ratchet's attach bone. Called once Ratchet's model is ready.
pub fn spawn(commands: &mut Commands, assets: &AssetServer, ratchet_attach_bone: Entity) {
    commands.insert_resource(ClankGltf(assets.load(MODEL_FILE)));
    let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(MODEL_FILE));
    let clank = commands.spawn((Name::new("Clank"), WorldAssetRoot(scene), Transform::default())).observe(on_ready).id();
    commands.entity(ratchet_attach_bone).add_child(clank);
}

fn on_ready(
    ev: On<WorldInstanceReady>,
    mut rig: ResMut<ClankRig>,
    children: Query<&Children>,
    nodes: Query<(&Transform, Option<&Name>)>,
    parents: Query<&ChildOf>,
    players: Query<(), With<AnimationPlayer>>,
    extras: Query<&GltfExtras>,
    mut commands: Commands,
) {
    let root = ev.entity;
    rig.heli_parts = children
        .iter_descendants(root)
        .filter(|e| extras.get(*e).is_ok_and(|x| x.value.replace(' ', "").contains("\"part\":\"heli\"")))
        .collect();
    for &e in &rig.heli_parts {
        commands.entity(e).insert(Visibility::Hidden);
    }
    rig.heli_shown = false;
    let mut by_name = HashMap::new();
    for e in children.iter_descendants(root) {
        if let Ok((_, Some(name))) = nodes.get(e) {
            by_name.entry(name.as_str().to_string()).or_insert(e);
        }
    }
    let attach = by_name.get(ATTACH_JOINT).copied();
    // Spin axis per rotor: blades A and B sit 120 degrees apart around it, so it's the axis of
    // the rotation between their rest frames (in the spin joint's space).
    let in_space_of = |child: Entity, ancestor: Entity| -> Option<Transform> {
        let mut chain = vec![];
        let mut e = child;
        while e != ancestor {
            chain.push(e);
            e = parents.get(e).ok()?.parent();
        }
        let mut t = Transform::IDENTITY;
        for e in chain.into_iter().rev() {
            t = t * *nodes.get(e).ok()?.0;
        }
        Some(t)
    };
    rig.rotors.clear();
    for prefix in ROTORS {
        let get = |suffix: &str| by_name.get(&format!("{prefix}{suffix}")).copied();
        if let (Some(spin), Some(a), Some(b)) = (get("BladeSpin"), get("BladeA"), get("BladeB"))
            && let (Some(ta), Some(tb)) = (in_space_of(a, spin), in_space_of(b, spin))
        {
            let (axis, angle) = (tb.rotation * ta.rotation.inverse()).to_axis_angle();
            if angle.abs() > 0.1 {
                rig.rotors.push((spin, axis.normalize()));
            }
        }
    }
    rig.player = children.iter_descendants(root).find(|e| players.contains(*e));
    rig.root = Some(root);
    rig.attach = attach;
    let Some(mut e) = attach else {
        warn!("clank model: no {ATTACH_JOINT} joint; leaving him at the attach bone's origin");
        return;
    };
    // Rest pose of Clank's attach joint in his model space...
    let mut chain = vec![];
    while e != root {
        chain.push(e);
        match parents.get(e) {
            Ok(p) => e = p.parent(),
            Err(_) => break,
        }
    }
    let mut attach_in_model = Transform::IDENTITY;
    for e in chain.into_iter().rev() {
        if let Ok((t, _)) = nodes.get(e) {
            attach_in_model = attach_in_model * *t;
        }
    }
    // ...so offset his root by its inverse: the attach joint then sits exactly on Ratchet's.
    commands.entity(root).insert(Transform::from_matrix(attach_in_model.to_matrix().inverse()));
    info!(
        "clank attached (offset {:?}), {} rotors, {} heli-pack meshes",
        attach_in_model.translation,
        rig.rotors.len(),
        rig.heli_parts.len()
    );
}

/// Keeps Clank's (animated) attach joint exactly on Ratchet's ClankAttach bone: his root's
/// local transform is the inverse of the attach joint relative to the root (last frame's pose).
fn follow_attach(rig: Res<ClankRig>, globals: Query<&GlobalTransform>, mut transforms: Query<&mut Transform>) {
    let (Some(root), Some(attach)) = (rig.root, rig.attach) else { return };
    let (Ok(root_global), Ok(attach_global)) = (globals.get(root), globals.get(attach)) else { return };
    let relative = root_global.affine().inverse() * attach_global.affine();
    if let Ok(mut t) = transforms.get_mut(root) {
        *t = Transform::from_matrix(Mat4::from(relative.inverse()));
    }
}

/// Spins the heli rotors while the heli-pack is out. Runs after the animation is applied, on
/// top of the clip's pose.
fn spin_rotors(time: Res<Time>, mut rig: ResMut<ClankRig>, mut transforms: Query<&mut Transform>) {
    if !rig.heli_shown {
        return;
    }
    rig.rotor_angle = (rig.rotor_angle + ROTOR_SPEED * time.delta_secs()) % std::f32::consts::TAU;
    let spin = rig.rotor_angle;
    for &(entity, axis) in &rig.rotors {
        if let Ok(mut t) = transforms.get_mut(entity) {
            t.rotation *= Quat::from_axis_angle(axis, spin);
        }
    }
}

/// Shows the heli-pack looks while it's out (extend, glide, retract), hides them otherwise.
fn show_heli(mut rig: ResMut<ClankRig>, mut visibility: Query<&mut Visibility>) {
    let want = rig.current.is_some_and(|c| HELI_CLIPS.contains(&c));
    if want == rig.heli_shown {
        return;
    }
    rig.heli_shown = want;
    for &e in &rig.heli_parts {
        if let Ok(mut v) = visibility.get_mut(e) {
            *v = if want { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

fn setup_clips(
    mut commands: Commands,
    mut rig: ResMut<ClankRig>,
    model: Option<Res<ClankGltf>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let (Some(player), Some(model)) = (rig.player, model) else { return };
    if !rig.clips.is_empty() {
        return;
    }
    let Some(gltf) = gltfs.get(&model.0) else { return };
    let mut graph = AnimationGraph::new();
    for &name in CLIPS {
        if let Some(clip) = gltf.named_animations.get(name) {
            let node = graph.add_clip(clip.clone(), 1.0, graph.root);
            rig.clips.insert(name, node);
        }
    }
    if rig.clips.is_empty() {
        rig.player = None;
        return;
    }
    commands.entity(player).insert((AnimationGraphHandle(graphs.add(graph)), AnimationTransitions::new()));
}

fn play_clips(
    time: Res<Time>,
    mut rig: ResMut<ClankRig>,
    players: Query<&Player>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let (Some(entity), Ok(p)) = (rig.player, players.single()) else { return };
    if rig.clips.is_empty() {
        return;
    }
    let Ok((mut player, mut transitions)) = anim.get_mut(entity) else { return };
    rig.one_shot = (rig.one_shot - time.delta_secs()).max(0.0);

    let gliding = p.sim.state() == MoveState::Glide;
    let mut want = rig.current.unwrap_or("idle");
    if gliding && !rig.gliding {
        want = "heli_extend";
        rig.one_shot = 0.23;
    } else if !gliding && rig.gliding {
        want = "heli_retract";
        rig.one_shot = 0.3;
    } else if rig.one_shot <= 0.0 {
        want = if gliding { "glide" } else { "idle" };
    }
    rig.gliding = gliding;

    let want = if rig.clips.contains_key(want) { want } else if gliding { "glide" } else { "idle" };
    if rig.current != Some(want)
        && let Some(&node) = rig.clips.get(want)
    {
        let active = transitions.play(&mut player, node, Duration::from_millis(100));
        if want == "idle" || want == "glide" {
            active.repeat();
        }
        rig.current = Some(want);
    }
}
