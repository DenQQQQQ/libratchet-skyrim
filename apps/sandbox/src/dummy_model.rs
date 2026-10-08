//! The combat dummies wear the game's test-dummy enemy (`enm_testdummy_ps4`) when
//! `riftapart-assets/testdummy.glb` exists (made from the user's own install by
//! `tools/extract_riftapart_ratchet.py`). They play its idle and its hit, knockback and death
//! reactions, and turn to face Ratchet. Without the file they stay capsules.

use crate::combat::Dummy;
use crate::{Player, Settings};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady};
use std::collections::HashMap;
use std::time::Duration;

pub const MODEL_FILE: &str = "testdummy.glb";
/// The model's height (metres): the dummies' hit volume grows to match.
pub const MODEL_HEIGHT: f32 = 1.9;
const CLIPS: &[&str] = &["idle", "hit", "knockback", "knockdown", "death", "taunt"];
/// How fast a dummy turns to face Ratchet (rad/s; ours).
const TURN_RATE: f32 = 4.0;

#[derive(Resource)]
struct DummyModel {
    gltf: Handle<Gltf>,
    scene: Handle<WorldAsset>,
    graph: Option<(Handle<AnimationGraph>, HashMap<&'static str, AnimationNodeIndex>)>,
}

/// One dummy's model: its instance, the entity with its AnimationPlayer once it's spawned, the
/// clip playing, and how long it had left stunned last frame (a jump up means a new hit).
#[derive(Component)]
pub struct DummyLook {
    model: Entity,
    player: Option<Entity>,
    clip: Option<&'static str>,
    stun_seen: f32,
    yaw: f32,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, setup.after(crate::combat::spawn_dummies)).add_systems(Update, (build_graph, react).chain());
}

fn setup(mut commands: Commands, settings: Res<Settings>, assets: Res<AssetServer>, mut dummies: Query<(Entity, &mut Dummy, &Children)>) {
    if !settings.model_dir.as_ref().is_some_and(|d| d.join(MODEL_FILE).exists()) {
        return;
    }
    let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(MODEL_FILE));
    for (entity, mut dummy, parts) in &mut dummies {
        // The capsule parts stay for nothing but their place in the hierarchy.
        for &part in parts {
            commands.entity(part).insert(Visibility::Hidden);
        }
        dummy.wear_model(MODEL_HEIGHT);
        let model = commands.spawn((WorldAssetRoot(scene.clone()), Transform::default())).observe(on_ready).id();
        commands.entity(entity).add_child(model).insert(DummyLook { model, player: None, clip: None, stun_seen: 0.0, yaw: 0.0 });
    }
    commands.insert_resource(DummyModel { gltf: assets.load(MODEL_FILE), scene, graph: None });
    info!("combat dummies: the game's test dummy ({MODEL_FILE})");
}

fn on_ready(
    ev: On<WorldInstanceReady>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    mut looks: Query<&mut DummyLook>,
) {
    let Some(player) = children.iter_descendants(ev.entity).find(|e| players.contains(*e)) else { return };
    if let Ok(parent) = parents.get(ev.entity)
        && let Ok(mut look) = looks.get_mut(parent.parent())
    {
        // A scene spawned again (see clank.rs): the new player has no clip playing yet.
        if look.player != Some(player) {
            look.clip = None;
        }
        look.player = Some(player);
    }
}

/// Builds the shared animation graph once the glTF is loaded, and gives it to each dummy's
/// player as it appears.
fn build_graph(
    mut commands: Commands,
    model: Option<ResMut<DummyModel>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    looks: Query<&DummyLook>,
    has_graph: Query<(), With<AnimationGraphHandle>>,
) {
    let Some(mut model) = model else { return };
    if model.graph.is_none() {
        let Some(gltf) = gltfs.get(&model.gltf) else { return };
        let mut graph = AnimationGraph::new();
        let nodes: HashMap<_, _> =
            CLIPS.iter().filter_map(|&name| gltf.named_animations.get(name).map(|clip| (name, graph.add_clip(clip.clone(), 1.0, graph.root)))).collect();
        model.graph = Some((graphs.add(graph), nodes));
    }
    let Some((graph, _)) = &model.graph else { return };
    for look in &looks {
        if let Some(player) = look.player
            && !has_graph.contains(player)
        {
            commands.entity(player).insert((AnimationGraphHandle(graph.clone()), AnimationTransitions::new()));
        }
    }
    let _ = &model.scene;
}

/// Picks each dummy's reaction: hit or knocked back when struck, dying when its health runs
/// out, idle otherwise; and turns it to face Ratchet.
fn react(
    time: Res<Time>,
    model: Option<Res<DummyModel>>,
    ratchet: Query<&Player>,
    mut dummies: Query<(&Dummy, &mut DummyLook)>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    mut transforms: Query<&mut Transform>,
) {
    let Some((_, nodes)) = model.as_ref().and_then(|m| m.graph.as_ref()) else { return };
    let target = ratchet.single().map(|p| p.feet).ok();
    let dt = time.delta_secs();
    for (k, (d, mut look)) in dummies.iter_mut().enumerate() {
        let Some(Ok((mut player, mut transitions))) = look.player.map(|p| anim.get_mut(p)) else { continue };
        let struck = d.stun() > look.stun_seen + 1e-3;
        look.stun_seen = d.stun();
        let finished = |clip: Option<&'static str>, player: &AnimationPlayer| clip.and_then(|c| nodes.get(c)).is_none_or(|&n| player.animation(n).is_none_or(|a| a.is_finished()));
        let want: Option<&'static str> = if d.is_down() {
            (look.clip != Some("death")).then_some("death")
        } else if struck {
            Some(if d.velocity().length() > 3.0 { "knockback" } else { "hit" })
        } else if look.clip != Some("idle") && (look.clip == Some("death") || finished(look.clip, &player)) {
            Some("idle")
        } else {
            None
        };
        if let Some(clip) = want
            && let Some(&node) = nodes.get(clip)
        {
            let fade = if clip == "idle" { 250 } else { 60 };
            let active = transitions.play(&mut player, node, Duration::from_millis(fade));
            if clip == "idle" {
                // Each at its own pace, so they drift out of step.
                active.repeat().set_speed(0.9 + 0.05 * (k % 5) as f32);
            } else {
                active.replay();
            }
            look.clip = Some(clip);
        }
        // Face Ratchet (not while dying), turning at a steady rate.
        if let Some(t) = target
            && !d.is_down()
        {
            let to = t - d.pos;
            if to.x.abs() + to.z.abs() > 1e-3 {
                let want_yaw = to.x.atan2(to.z);
                let mut diff = want_yaw - look.yaw;
                while diff > std::f32::consts::PI {
                    diff -= std::f32::consts::TAU;
                }
                while diff < -std::f32::consts::PI {
                    diff += std::f32::consts::TAU;
                }
                look.yaw += diff.clamp(-TURN_RATE * dt, TURN_RATE * dt);
            }
        }
        if let Ok(mut tf) = transforms.get_mut(look.model) {
            tf.rotation = Quat::from_rotation_y(look.yaw);
        }
    }
}
