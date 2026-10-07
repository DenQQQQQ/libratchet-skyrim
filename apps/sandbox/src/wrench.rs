//! The OmniWrench, when `riftapart-assets/wrench.glb` exists: held in Ratchet's right hand
//! (`RT_handle_bound`, the game's gun-attach locator) while he swings (playing its own squash-and-stretch clips in time with his), and thrown
//! with aim + melee like the game (right mouse held, then E or left mouse), or Q, the game's
//! one-button "Melee Throw" shortcut (libratchet's `WrenchThrow`), knocking back the first dummy
//! it meets.

use crate::combat::{Dummy, Hits, knock};
use crate::level::Level;
use crate::vfx::{FxRng, VfxArt};
use crate::{CameraRig, Player, Settings, simulate};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use libratchet::{MoveState, ThrowStatus, WrenchThrow};
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

pub const MODEL_FILE: &str = "wrench.glb";
const CLIPS: &[&str] = &["melee_1", "melee_2", "melee_3"];
/// The wrench stays in hand this long after a swing or a catch.
const SHOW_AFTER: f32 = 0.35;
/// Ratchet's throw animation (until the catch).
const THROW_ANIM: f32 = 1.4;

#[derive(Resource)]
struct WrenchGltf(Handle<Gltf>);

/// A throw requested by the demo script, used on the next tick.
#[derive(Resource, Default)]
pub struct ScriptedThrow(pub bool);

#[derive(Resource, Default)]
pub struct Wrench {
    /// Ratchet's hand joint, the held copy's holder and its AnimationPlayer.
    hand: Option<Entity>,
    held: Option<Entity>,
    player: Option<Entity>,
    clips: HashMap<&'static str, AnimationNodeIndex>,
    /// The swing whose clip is playing.
    playing: Option<usize>,
    show: f32,
    /// The thrown copy's holder and its flight.
    thrown: Option<Entity>,
    pub flight: Option<WrenchThrow>,
    throw_requested: bool,
    /// A throw whose animation has started: seconds until the wrench leaves the hand, and
    /// where it goes.
    releasing: Option<(f32, Vec3)>,
    /// Seconds left of Ratchet's throw animation.
    pub throw_anim: f32,
    /// Counts throws, so the animation can restart its layer for each one, and catches.
    pub throws: u32,
    pub catches: u32,
}

impl Wrench {
    /// The held wrench's holder (it carries the wrench model's transform), for its effects.
    pub fn held(&self) -> Option<Entity> {
        self.held
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Wrench>()
        .init_resource::<ScriptedThrow>()
        .add_systems(FixedUpdate, (start_throw, fly).chain().after(simulate))
        .add_systems(Update, (throw_button, setup_clips, held_wrench, thrown_visuals).chain());
}

/// Spawns the held wrench in Ratchet's hand, and a second copy for throwing.
pub fn spawn(commands: &mut Commands, assets: &AssetServer, hand: Entity) {
    commands.insert_resource(WrenchGltf(assets.load(MODEL_FILE)));
    let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(MODEL_FILE));
    // Each copy sits under a holder we own: spawning the scene takes over its root entity's
    // components, so visibility and placement go on the holder.
    let held = commands.spawn((Name::new("Wrench"), Transform::default(), Visibility::Hidden)).id();
    commands.entity(hand).add_child(held);
    let held_scene = commands.spawn((WorldAssetRoot(scene.clone()), Transform::default())).observe(on_held_ready).id();
    commands.entity(held).add_child(held_scene);
    let thrown = commands.spawn((Name::new("Thrown wrench"), Transform::default(), Visibility::Hidden)).id();
    let thrown_scene = commands.spawn((WorldAssetRoot(scene), Transform::default())).observe(inherit_visibility).id();
    commands.entity(thrown).add_child(thrown_scene);
    commands.insert_resource(Wrench { hand: Some(hand), held: Some(held), thrown: Some(thrown), ..default() });
}

fn on_held_ready(
    ev: On<WorldInstanceReady>,
    mut wrench: ResMut<Wrench>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    mut commands: Commands,
) {
    wrench.player = children.iter_descendants(ev.entity).find(|e| players.contains(*e));
    commands.entity(ev.entity).insert(Visibility::Inherited);
}

/// The spawned scene's root can come out explicitly visible; make it follow its holder.
fn inherit_visibility(ev: On<WorldInstanceReady>, mut commands: Commands) {
    commands.entity(ev.entity).insert(Visibility::Inherited);
}

fn setup_clips(
    mut commands: Commands,
    mut wrench: ResMut<Wrench>,
    model: Option<Res<WrenchGltf>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let (Some(player), Some(model)) = (wrench.player, model) else { return };
    if !wrench.clips.is_empty() {
        return;
    }
    let Some(gltf) = gltfs.get(&model.0) else { return };
    let mut graph = AnimationGraph::new();
    for &name in CLIPS {
        if let Some(clip) = gltf.named_animations.get(name) {
            let node = graph.add_clip(clip.clone(), 1.0, graph.root);
            wrench.clips.insert(name, node);
        }
    }
    if wrench.clips.is_empty() {
        wrench.player = None;
        return;
    }
    commands.entity(player).insert((AnimationGraphHandle(graphs.add(graph)), AnimationTransitions::new()));
}

/// Shows the held wrench during melee (and briefly after), hidden while it's thrown; starts the
/// wrench's clip with each of Ratchet's swings.
fn held_wrench(
    time: Res<Time>,
    mut wrench: ResMut<Wrench>,
    players: Query<&Player>,
    mut visibility: Query<&mut Visibility>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Ok(p) = players.single() else { return };
    let in_melee = matches!(p.sim.state(), MoveState::Melee | MoveState::Smash | MoveState::SmashLand);
    wrench.show = if in_melee { SHOW_AFTER } else { (wrench.show - time.delta_secs()).max(0.0) };
    wrench.throw_anim = (wrench.throw_anim - time.delta_secs()).max(0.0);
    let want = wrench.flight.is_none() && wrench.show > 0.0;
    if let Some(held) = wrench.held
        && let Ok(mut v) = visibility.get_mut(held)
    {
        v.set_if_neq(if want { Visibility::Inherited } else { Visibility::Hidden });
    }
    let swing = p.sim.melee_swing();
    if swing != wrench.playing {
        wrench.playing = swing;
        if let (Some(i), Some(player)) = (swing, wrench.player)
            && let Some(&node) = CLIPS.get(i).and_then(|c| wrench.clips.get(c))
            && let Ok((mut player, mut transitions)) = anim.get_mut(player)
        {
            transitions.play(&mut player, node, Duration::from_millis(50));
        }
    }
}

/// Melee while aiming asks for a throw, as in the game ("Hold [Aim] and [Melee] to Throw
/// Wrench"): E, or left mouse with the wrench selected. Q is the game's optional one-button
/// "Melee Throw" shortcut.
fn throw_button(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    arsenal: Option<Res<crate::weapons::Arsenal>>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    mut wrench: ResMut<Wrench>,
) {
    let grabbed = cursor.single().is_ok_and(|c| c.grab_mode != CursorGrabMode::None);
    let wrench_out = arsenal.as_ref().is_none_or(|a| a.slot == crate::weapons::Slot::Wrench);
    let aiming = arsenal.as_ref().is_some_and(|a| a.aiming);
    let melee = keys.just_pressed(KeyCode::KeyE) || (grabbed && wrench_out && mouse.just_pressed(MouseButton::Left));
    if keys.just_pressed(KeyCode::KeyQ) || (aiming && melee) {
        wrench.throw_requested = true;
    }
}

fn hand_position(wrench: &Wrench, globals: &Query<&GlobalTransform>, p: &Player, scale: f32) -> Vec3 {
    wrench.hand.and_then(|h| globals.get(h).ok()).map_or(p.feet + Vec3::Y * 0.9 * scale, |g| g.translation())
}

/// Throws along the camera (the demo throws straight ahead), turning Ratchet to face it; the
/// wrench leaves the hand partway into the animation.
#[allow(clippy::too_many_arguments)]
fn start_throw(
    time: Res<Time>,
    settings: Res<Settings>,
    rig: Res<CameraRig>,
    mut scripted: ResMut<ScriptedThrow>,
    mut wrench: ResMut<Wrench>,
    globals: Query<&GlobalTransform>,
    mut players: Query<&mut Player>,
) {
    let from_script = std::mem::take(&mut scripted.0);
    let requested = std::mem::take(&mut wrench.throw_requested) || from_script;
    let Ok(mut p) = players.single_mut() else { return };
    // The wrench leaves the hand at the throw clip's WeaponThrown event.
    if let Some((left, dir)) = wrench.releasing {
        let left = left - time.delta_secs();
        if left > 0.0 {
            wrench.releasing = Some((left, dir));
        } else {
            wrench.releasing = None;
            let from = hand_position(&wrench, &globals, &p, settings.scale);
            wrench.flight = Some(WrenchThrow::new(&p.sim.tuning().melee.throw, from, dir));
        }
    }
    let busy = wrench.flight.is_some() || wrench.releasing.is_some();
    if !requested || busy || matches!(p.sim.state(), MoveState::Melee | MoveState::Smash | MoveState::SmashLand) {
        return;
    }
    let dir = if from_script { p.sim.facing_dir() } else { Vec3::new(-rig.yaw.sin(), 0.0, -rig.yaw.cos()) };
    p.sim.set_facing_yaw(dir.x.atan2(dir.z));
    wrench.releasing = Some((p.sim.tuning().melee.throw.release, dir));
    wrench.show = SHOW_AFTER;
    wrench.throw_anim = THROW_ANIM;
    wrench.throws += 1;
}

/// Flies the thrown wrench: it turns back at a dummy (knocking it back) or a wall, and is
/// caught at the hand. It leaves the game's blue trail, and hits spark like a swing's.
#[allow(clippy::too_many_arguments)]
fn fly(
    time: Res<Time>,
    settings: Res<Settings>,
    level: Res<Level>,
    mut wrench: ResMut<Wrench>,
    globals: Query<&GlobalTransform>,
    players: Query<&Player>,
    mut dummies: Query<&mut Dummy>,
    mut hits: ResMut<Hits>,
    (art, mut rng): (Res<VfxArt>, ResMut<FxRng>),
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let Ok(p) = players.single() else { return };
    let hand = hand_position(&wrench, &globals, p, settings.scale);
    let s = settings.scale;
    let Some(flight) = wrench.flight.as_mut() else { return };
    let throw = p.sim.tuning().melee.throw.clone();
    let pos = flight.position();
    let hit_at = knock(&mut dummies, pos, throw.damage_radius, pos - flight.direction(), 2.0 * s, 1.0 * s, throw.damage, |d| d.pos.y < 0.5 && hit_ready(d));
    let n = hit_at.len();
    if art.available {
        for &(at, floor) in &hit_at {
            let from = pos - flight.direction();
            art.wrench_hit(&mut commands, &mut materials, &mut rng, at, (at - from).with_y(0.0).normalize_or(Vec3::X), Some(floor), s);
        }
    }
    if n > 0 {
        hits.count += n as u32;
        hits.last = format!("thrown wrench hit {n} ({} damage)", throw.damage);
    }
    // The floor is block 0; anything else stops the wrench.
    let wall = level.blocks.iter().skip(1).any(|b| pos.cmpge(b.min).all() && pos.cmple(b.max).all());
    let status = flight.step(time.delta_secs(), hand, n > 0 || wall);
    if art.available {
        art.wrench_trail(&mut commands, &mut materials, pos, flight.position(), s);
    }
    if status == ThrowStatus::Caught {
        wrench.flight = None;
        wrench.catches += 1;
        wrench.show = SHOW_AFTER;
        wrench.throw_anim = wrench.throw_anim.min(0.3);
    }
}

/// A dummy that wasn't just hit.
fn hit_ready(d: &Dummy) -> bool {
    d.since_hit() > 0.5
}

fn thrown_visuals(settings: Res<Settings>, wrench: Res<Wrench>, mut q: Query<(&mut Transform, &mut Visibility)>) {
    let Some(entity) = wrench.thrown else { return };
    let Ok((mut tf, mut vis)) = q.get_mut(entity) else { return };
    match &wrench.flight {
        Some(f) => {
            // Lying flat, spinning like a boomerang.
            *tf = Transform::from_translation(f.position())
                .with_rotation(Quat::from_rotation_y(f.spin()) * Quat::from_rotation_z(FRAC_PI_2))
                .with_scale(Vec3::splat(settings.scale));
            *vis = Visibility::Inherited;
        }
        None => *vis = Visibility::Hidden,
    }
}
