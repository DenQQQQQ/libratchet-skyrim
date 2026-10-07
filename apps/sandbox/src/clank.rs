//! Clank on Ratchet's back, when `riftapart-assets/clank.glb` exists (converted from the user's
//! own install by `tools/extract_riftapart_ratchet.py`).
//!
//! Like the game, Clank's `dyn_backpack_attach` joint is placed on Ratchet's `ClankAttach` joint
//! (both carry the `igLoc_clankBackpack` locator). He plays his backpack idle, and his broken
//! heli-pack extends while Ratchet glides and retracts afterwards.

use crate::Player;
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use libratchet::MoveState;
use std::collections::HashMap;
use std::time::Duration;

pub const MODEL_FILE: &str = "clank.glb";
const ATTACH_JOINT: &str = "dyn_backpack_attach";
const CLIPS: &[&str] = &["idle", "glide", "heli_extend", "heli_retract"];

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
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ClankRig>().add_systems(Update, (setup_clips, play_clips, follow_attach).chain());
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
    mut commands: Commands,
) {
    let root = ev.entity;
    let mut attach = None;
    for e in children.iter_descendants(root) {
        if let Ok((_, Some(name))) = nodes.get(e)
            && name.as_str() == ATTACH_JOINT
        {
            attach = Some(e);
            break;
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
    info!("clank attached (offset {:?})", attach_in_model.translation);
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
