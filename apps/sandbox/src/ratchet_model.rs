//! Shows Ratchet's real model on the player when `riftapart-assets/ratchet.glb` exists (made
//! from the user's own Rift Apart install by `tools/extract_riftapart_ratchet.py`). When the file
//! carries the game's animation clips (idle, run, jump...), they're played by move state with
//! crossfades; otherwise the skeleton is posed procedurally.
//!
//! Bone rotations are given in the model's rest-pose space and applied as
//! `local = parentRest⁻¹ · R · parentRest · localRest`, which bends each joint around an axis
//! that stays attached to its parent (a proper hinge), whatever the rig's local axes are.

use crate::{Player, Settings};
use bevy::animation::AnimationTargetId;
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use libratchet::MoveState;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::path::PathBuf;
use std::time::Duration;

pub const MODEL_FILE: &str = "ratchet.glb";

/// Finds `riftapart-assets/` (with the converted model) in the working directory, next to the
/// executable, or any folder above them.
pub fn find_assets_dir() -> Option<PathBuf> {
    let starts = [std::env::current_dir().ok(), std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from))];
    for start in starts.into_iter().flatten() {
        let mut dir = start;
        loop {
            let cand = dir.join("riftapart-assets");
            if cand.join(MODEL_FILE).exists() {
                return Some(cand);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

/// Marks the capsule placeholder parts, hidden once the real model is in.
#[derive(Component)]
pub struct Placeholder;

#[derive(Component)]
struct ModelRoot;

#[derive(Resource)]
struct ModelGltf(Handle<Gltf>);

/// Clip names baked into ratchet.glb by the converter (one per move).
const CLIP_STATES: &[&str] = &[
    "idle", "run", "jump", "double_jump", "fall", "glide", "land", "dash", "air_dash", "melee_1", "melee_2", "melee_3", "smash_intro", "smash",
    "aim_idle", "strafe_n", "strafe_ne", "strafe_e", "strafe_se", "strafe_s", "strafe_sw", "strafe_w", "strafe_nw", "grenade_throw",
    "pistol_idle", "pistol_run", "walk", "land_walk", "land_run", "idle_look", "idle_shoe",
];
/// The run's lean-into-a-turn versions (left, right), blended in step with it by how fast he
/// turns.
const LEAN_CLIPS: [&str; 2] = ["run_lean_left", "run_lean_right"];
/// Turn rate (rad/s) at which the run leans fully into the turn (ours).
const FULL_LEAN_TURN_RATE: f32 = 5.0;
/// Walk below this speed, run above that one, with some overlap (m/s at Ratchet's size; between
/// the game's walk and run speed bands; ours).
const RUN_TO_WALK: f32 = 2.4;
const WALK_TO_RUN: f32 = 3.0;
/// Idle fidgets, after standing still this long (ours).
const FIDGETS: &[&str] = &["idle_look", "idle_shoe"];
const FIDGET_AFTER: f32 = 7.0;
/// Catching the thrown wrench: a full-body clip of which only the upper body (the `spine_1`
/// branch) is used, weighted well over the move clip under it.
const CATCH_CLIP: &str = "throw_catch";
const CATCH_WEIGHT: f32 = 6.0;
/// The game's arm-only grenade throw, layered (like the catch) over walking, strafing or a jump;
/// standing still, the full-body throw plays instead.
const GRENADE_ARM_CLIP: &str = "grenade_throw_arm";
/// Strafing while aiming, by direction relative to where Ratchet faces (clockwise from ahead).
const STRAFE_CLIPS: &[&str] = &["strafe_n", "strafe_ne", "strafe_e", "strafe_se", "strafe_s", "strafe_sw", "strafe_w", "strafe_nw"];
/// The one-handed fire recoil: an additive layer, restarted with every shot (when the model has
/// one; the converter leaves it out for now).
const FIRE_LAYER: &str = "fire_pistol_additive";
/// The wrench throw is an additive clip in the game (offsets on top of whatever Ratchet is
/// doing), so it plays as a layer over the move clips.
const THROW_LAYER: &str = "throw_additive";
/// Fade in/out of the throw layer, seconds.
const LAYER_FADE_IN: f32 = 0.08;
const LAYER_FADE_OUT: f32 = 0.25;
const MELEE_CLIPS: &[&str] = &["melee_1", "melee_2", "melee_3"];
const LOOPING: &[&str] = &[
    "idle", "run", "fall", "glide", "aim_idle", "strafe_n", "strafe_ne", "strafe_e", "strafe_se", "strafe_s", "strafe_sw", "strafe_w", "strafe_nw",
    "pistol_idle", "pistol_run", "walk",
];

struct Bone {
    entity: Entity,
    rest_local: Quat,
    parent_rest: Quat,
}

#[derive(Resource)]
pub struct Rig {
    bones: HashMap<&'static str, Bone>,
    /// Rest-pose angle of each upper arm in the model's XY plane (radians), to lower the arms
    /// from whatever pose the model was authored in.
    arm_rest_angle: [f32; 2],
    pose: Pose,
    phase: f32,
    /// Entity with the glTF AnimationPlayer (the skeleton root), when the model has clips.
    player: Option<Entity>,
    clips: HashMap<&'static str, AnimationNodeIndex>,
    current_clip: Option<&'static str>,
    prev_state: Option<MoveState>,
    land_timer: f32,
    /// The additive throw layer: its node, length, how long it has played, and the throw count
    /// it belongs to.
    throw_layer: Option<(AnimationNodeIndex, f32)>,
    throw_time: Option<f32>,
    throws_seen: u32,
    fire_layer: Option<(AnimationNodeIndex, f32)>,
    fire_time: Option<f32>,
    shots_seen: u32,
    /// The speed each clip was authored for (its root motion over its length, from the
    /// converter's ratchet.json), to play it at the rate that keeps the feet planted.
    authored_speed: HashMap<String, f32>,
    walking: bool,
    /// The run's lean: −1 (right) … 1 (left), its two clips, and the yaw it was last measured at.
    lean: f32,
    lean_nodes: Option<[AnimationNodeIndex; 2]>,
    lean_blend: f32,
    prev_yaw: Option<f32>,
    /// The wrench catch (upper body), how long it has played, and the catch count it belongs to.
    catch_layer: Option<(AnimationNodeIndex, f32)>,
    catch_time: Option<f32>,
    catches_seen: u32,
    /// The arm-only grenade throw, how long it has played, and the throw count it belongs to.
    grenade_layer: Option<(AnimationNodeIndex, f32)>,
    grenade_time: Option<f32>,
    grenades_seen: u32,
    /// Joints outside the upper body, masked out of the catch.
    lower_body: Vec<Entity>,
    idle_time: f32,
    fidgets_played: usize,
}

const BONES: &[&str] = &[
    "pelvis", "spine_1", "spine_2", "spine_3", "neck_01", "head", "LF_upleg", "RT_upleg", "LF_loleg", "RT_loleg", "LF_uparm", "RT_uparm",
    "LF_loarm", "RT_loarm", "LF_ear_1", "RT_ear_1", "tail_1", "tail_2", "tail_3", "tail_4", "tail_5", "tail_6",
];

/// Pose parameters in radians. Legs: + swings back. Spine: + leans forward.
/// Arm drop: 0 = straight out to the side, π/2 = straight down.
#[derive(Clone, Copy, Default)]
struct Pose {
    hip: [f32; 2],
    knee: [f32; 2],
    arm_drop: f32,
    arm_swing: [f32; 2],
    elbow: f32,
    lean: f32,
    head: f32,
    ears: f32,
    tail_lift: f32,
    tail_sway: f32,
}

impl Pose {
    fn lerp(&self, o: &Pose, t: f32) -> Pose {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Pose {
            hip: [l(self.hip[0], o.hip[0]), l(self.hip[1], o.hip[1])],
            knee: [l(self.knee[0], o.knee[0]), l(self.knee[1], o.knee[1])],
            arm_drop: l(self.arm_drop, o.arm_drop),
            arm_swing: [l(self.arm_swing[0], o.arm_swing[0]), l(self.arm_swing[1], o.arm_swing[1])],
            elbow: l(self.elbow, o.elbow),
            lean: l(self.lean, o.lean),
            head: l(self.head, o.head),
            ears: l(self.ears, o.ears),
            tail_lift: l(self.tail_lift, o.tail_lift),
            tail_sway: l(self.tail_sway, o.tail_sway),
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (setup_clips, play_clips, animate).chain().after(crate::update_visuals));
}

/// Spawns the model under the player. Called from setup when the model file exists.
pub fn spawn(commands: &mut Commands, assets: &AssetServer, player: Entity) {
    commands.insert_resource(ModelGltf(assets.load(MODEL_FILE)));
    let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(MODEL_FILE));
    let model = commands.spawn((ModelRoot, WorldAssetRoot(scene), Transform::default())).observe(on_ready).id();
    commands.entity(player).add_child(model);
}

fn on_ready(
    ev: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    nodes: Query<(&Transform, Option<&Name>)>,
    parents: Query<&ChildOf>,
    placeholders: Query<Entity, With<Placeholder>>,
    players: Query<(), With<AnimationPlayer>>,
    assets: Res<AssetServer>,
    settings: Res<Settings>,
) {
    let root = ev.entity;
    let player = children.iter_descendants(root).find(|e| players.contains(*e));
    // Name -> entity for everything under the model.
    let mut by_name = HashMap::new();
    for e in children.iter_descendants(root) {
        if let Ok((_, Some(name))) = nodes.get(e) {
            by_name.entry(name.as_str().to_string()).or_insert(e);
        }
    }
    // Rest-pose rotation/translation of an entity in model space.
    let rest_global = |mut e: Entity| -> (Quat, Vec3) {
        let mut chain = vec![];
        while e != root {
            chain.push(e);
            match parents.get(e) {
                Ok(p) => e = p.parent(),
                Err(_) => break,
            }
        }
        let (mut rot, mut pos) = (Quat::IDENTITY, Vec3::ZERO);
        for e in chain.into_iter().rev() {
            if let Ok((t, _)) = nodes.get(e) {
                pos += rot * t.translation;
                rot *= t.rotation;
            }
        }
        (rot, pos)
    };

    // Clank rides on the ClankAttach bone, like in the game.
    if let (Some(dir), Some(&attach)) = (settings.model_dir.as_ref(), by_name.get("ClankAttach"))
        && dir.join(crate::clank::MODEL_FILE).exists()
    {
        crate::clank::spawn(&mut commands, &assets, attach);
    }
    // The OmniWrench and the guns go in his right hand: the game mounts weapons on the
    // igLoc_RT_gunAttach locator, which sits on RT_handle_bound with no offset (the weapon
    // models' origin is their grip).
    if let (Some(dir), Some(&hand)) = (settings.model_dir.as_ref(), by_name.get("RT_handle_bound")) {
        if dir.join(crate::wrench::MODEL_FILE).exists() {
            crate::wrench::spawn(&mut commands, &assets, hand);
        }
        if dir.join(crate::weapons::PISTOL_FILE).exists() {
            crate::weapons::spawn_gun(&mut commands, &assets, hand);
        }
    }

    let mut bones = HashMap::new();
    for &name in BONES {
        let Some(&e) = by_name.get(name) else {
            warn!("ratchet model: bone {name} not found");
            continue;
        };
        let parent_rest = parents.get(e).map(|p| rest_global(p.parent()).0).unwrap_or(Quat::IDENTITY);
        let rest_local = nodes.get(e).map(|(t, _)| t.rotation).unwrap_or(Quat::IDENTITY);
        bones.insert(name, Bone { entity: e, rest_local, parent_rest });
    }
    let arm_angle = |up: &str, lo: &str| -> f32 {
        match (by_name.get(up), by_name.get(lo)) {
            (Some(&u), Some(&l)) => {
                let d = rest_global(l).1 - rest_global(u).1;
                d.y.atan2(d.x)
            }
            _ => 0.0,
        }
    };
    let arm_rest_angle = [arm_angle("LF_uparm", "LF_loarm"), arm_angle("RT_uparm", "RT_loarm")];
    // The upper body is the spine_1 branch; the root, pelvis, legs and tail are the rest.
    let upper: std::collections::HashSet<Entity> =
        by_name.get("spine_1").map(|&s| std::iter::once(s).chain(children.iter_descendants(s)).collect()).unwrap_or_default();
    let lower_body = if upper.is_empty() { vec![] } else { children.iter_descendants(root).filter(|e| !upper.contains(e)).collect() };
    let authored_speed = settings.model_dir.as_ref().map(|d| authored_speeds(&d.join("ratchet.json"))).unwrap_or_default();
    info!("ratchet model ready: {} bones rigged, arm rest angles {:.0}°/{:.0}°", bones.len(), arm_rest_angle[0].to_degrees(), arm_rest_angle[1].to_degrees());
    commands.insert_resource(Rig {
        bones,
        arm_rest_angle,
        pose: Pose::default(),
        phase: 0.0,
        player,
        clips: HashMap::new(),
        current_clip: None,
        prev_state: None,
        land_timer: 0.0,
        throw_layer: None,
        throw_time: None,
        throws_seen: 0,
        fire_layer: None,
        fire_time: None,
        shots_seen: 0,
        authored_speed,
        walking: false,
        lean: 0.0,
        lean_nodes: None,
        lean_blend: 0.0,
        prev_yaw: None,
        catch_layer: None,
        catch_time: None,
        catches_seen: 0,
        grenade_layer: None,
        grenade_time: None,
        grenades_seen: 0,
        lower_body,
        idle_time: 0.0,
        fidgets_played: 0,
    });
    for e in &placeholders {
        commands.entity(e).insert(Visibility::Hidden);
    }
}

/// Builds the animation graph once the glTF (with its named clips) is loaded.
fn setup_clips(
    mut commands: Commands,
    rig: Option<ResMut<Rig>>,
    model: Option<Res<ModelGltf>>,
    gltfs: Res<Assets<Gltf>>,
    clip_assets: Res<Assets<AnimationClip>>,
    targets: Query<&AnimationTargetId>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let (Some(mut rig), Some(model)) = (rig, model) else { return };
    let Some(player) = rig.player else { return };
    if !rig.clips.is_empty() {
        return;
    }
    let Some(gltf) = gltfs.get(&model.0) else { return };
    // The move clips crossfade under `base`; layers are added on top of it.
    let mut graph = AnimationGraph::new();
    let layers = graph.add_additive_blend(1.0, graph.root);
    let base = graph.add_blend(1.0, layers);
    let mut clips = HashMap::new();
    for &name in CLIP_STATES {
        if let Some(clip) = gltf.named_animations.get(name) {
            clips.insert(name, graph.add_clip(clip.clone(), 1.0, base));
        }
    }
    if let [Some(left), Some(right)] = LEAN_CLIPS.map(|n| gltf.named_animations.get(n)) {
        rig.lean_nodes = Some([graph.add_clip(left.clone(), 1.0, base), graph.add_clip(right.clone(), 1.0, base)]);
    }
    if let Some(clip) = gltf.named_animations.get(CATCH_CLIP)
        && !rig.lower_body.is_empty()
    {
        // Mask group 1 is everything below the waist; the catch node leaves it alone.
        for e in &rig.lower_body {
            if let Ok(&id) = targets.get(*e) {
                graph.add_target_to_mask_group(id, 1);
            }
        }
        let duration = clip_assets.get(clip).map_or(0.33, |c| c.duration());
        rig.catch_layer = Some((graph.add_clip_with_mask(clip.clone(), 1 << 1, 1.0, base), duration));
    }
    if let Some(clip) = gltf.named_animations.get(GRENADE_ARM_CLIP) {
        // Only the arm's joints are in the clip; everything else keeps the move clip.
        let duration = clip_assets.get(clip).map_or(0.47, |c| c.duration());
        rig.grenade_layer = Some((graph.add_clip(clip.clone(), 1.0, base), duration));
    }
    let layer = THROW_LAYER;
    if let Some(clip) = gltf.named_animations.get(layer) {
        let duration = clip_assets.get(clip).map_or(2.0, |c| c.duration());
        rig.throw_layer = Some((graph.add_clip(clip.clone(), 1.0, layers), duration));
        info!("ratchet model: throw plays as an additive layer ({layer}, {duration:.2} s)");
    }
    if let Some(clip) = gltf.named_animations.get(FIRE_LAYER) {
        let duration = clip_assets.get(clip).map_or(0.33, |c| c.duration());
        rig.fire_layer = Some((graph.add_clip(clip.clone(), 1.0, layers), duration));
    }
    if clips.is_empty() {
        rig.player = None; // no clips in this file: stay procedural
        return;
    }
    info!("ratchet model: playing {} Rift Apart clips", clips.len());
    commands.entity(player).insert((AnimationGraphHandle(graphs.add(graph)), AnimationTransitions::new()));
    rig.clips = clips;
}

/// Picks the clip for the current move and crossfades to it.
#[allow(clippy::too_many_arguments)]
fn play_clips(
    time: Res<Time>,
    settings: Res<Settings>,
    rig: Option<ResMut<Rig>>,
    wrench: Res<crate::wrench::Wrench>,
    arsenal: Option<Res<crate::weapons::Arsenal>>,
    players: Query<&Player>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let (Some(mut rig), Ok(p)) = (rig, players.single()) else { return };
    let Some(entity) = rig.player else { return };
    if rig.clips.is_empty() {
        return;
    }
    let Ok((mut player, mut transitions)) = anim.get_mut(entity) else { return };

    // The pistol's recoil layer: restarted with each shot.
    let shots = arsenal.as_ref().map_or(0, |a| a.shots);
    if let Some((node, duration)) = rig.fire_layer {
        if shots != rig.shots_seen {
            rig.shots_seen = shots;
            rig.fire_time = Some(0.0);
            player.start(node);
        }
        if let Some(t) = rig.fire_time {
            let t = t + time.delta_secs();
            if t >= duration {
                player.stop(node);
                rig.fire_time = None;
            } else {
                let weight = (t / 0.03).min(1.0).min(((duration - t) / 0.12).max(0.0));
                if let Some(active) = player.animation_mut(node) {
                    active.set_weight(weight);
                }
                rig.fire_time = Some(t);
            }
        }
    }

    // Catching the wrench: the upper body reaches for it; the throw layer gives way.
    if let Some((node, duration)) = rig.catch_layer {
        if wrench.catches != rig.catches_seen {
            rig.catches_seen = wrench.catches;
            rig.catch_time = Some(0.0);
            player.start(node);
            if let (Some(t), Some((_, throw_duration))) = (rig.throw_time, rig.throw_layer) {
                rig.throw_time = Some(t.max(throw_duration - LAYER_FADE_OUT));
            }
        }
        if let Some(t) = rig.catch_time {
            let t = t + time.delta_secs();
            if t >= duration {
                player.stop(node);
                rig.catch_time = None;
            } else {
                let weight = CATCH_WEIGHT * (t / 0.05).min(1.0).min(((duration - t) / 0.12).max(0.0));
                if let Some(active) = player.animation_mut(node) {
                    active.set_weight(weight);
                }
                rig.catch_time = Some(t);
            }
        }
    }

    // A grenade thrown on the move or in the air: the arm-only throw over the move clip.
    let flat_speed = Vec2::new(p.sim.velocity().x, p.sim.velocity().z).length() / settings.scale;
    let standing = p.sim.state() == MoveState::Ground && flat_speed <= 0.6;
    if let Some((node, duration)) = rig.grenade_layer {
        let throws = arsenal.as_ref().map_or(0, |a| a.throws);
        if throws != rig.grenades_seen {
            rig.grenades_seen = throws;
            if !standing {
                rig.grenade_time = Some(0.0);
                player.start(node);
            }
        }
        if let Some(t) = rig.grenade_time {
            let t = t + time.delta_secs();
            if t >= duration {
                player.stop(node);
                rig.grenade_time = None;
            } else {
                let weight = CATCH_WEIGHT * (t / 0.04).min(1.0).min(((duration - t) / 0.1).max(0.0));
                if let Some(active) = player.animation_mut(node) {
                    active.set_weight(weight);
                }
                rig.grenade_time = Some(t);
            }
        }
    }

    // The throw layer: restart it with each throw, fade it in and out over its length.
    if let Some((node, duration)) = rig.throw_layer {
        if wrench.throws != rig.throws_seen {
            rig.throws_seen = wrench.throws;
            rig.throw_time = Some(0.0);
            player.start(node);
        }
        if let Some(t) = rig.throw_time {
            let t = t + time.delta_secs();
            let weight = (t / LAYER_FADE_IN).min(1.0).min(((duration - t) / LAYER_FADE_OUT).max(0.0));
            if t >= duration {
                player.stop(node);
                rig.throw_time = None;
            } else {
                if let Some(active) = player.animation_mut(node) {
                    active.set_weight(weight);
                }
                rig.throw_time = Some(t);
            }
        }
    }
    let v = p.sim.velocity() / settings.scale;
    let speed = Vec2::new(v.x, v.z).length();
    let state = p.sim.state();
    let airborne = |s: Option<MoveState>| matches!(s, Some(MoveState::Jump | MoveState::DoubleJump | MoveState::Fall | MoveState::Glide | MoveState::AirDash));
    if state == MoveState::Ground && airborne(rig.prev_state) {
        rig.land_timer = if speed > 0.6 { 0.35 } else { 0.3 };
    }
    rig.prev_state = Some(state);
    rig.land_timer = (rig.land_timer - time.delta_secs()).max(0.0);

    // Aiming (any weapon), or the pistol firing: the one-handed stance, strafing by direction.
    let pistol_out = arsenal.as_ref().is_some_and(|a| a.slot == crate::weapons::Slot::BurstPistol);
    let gun_aim = arsenal.as_ref().is_some_and(|a| a.aiming || (pistol_out && (a.trigger || a.stance > 0.0)));
    let throwing_grenade = arsenal.as_ref().is_some_and(|a| a.grenade_anim > 0.0) && rig.grenade_time.is_none();
    let strafe = {
        let yaw = p.sim.facing_yaw();
        let (fwd, right) = (Vec2::new(yaw.sin(), yaw.cos()), Vec2::new(-yaw.cos(), yaw.sin()));
        let flat = Vec2::new(v.x, v.z);
        let angle = flat.dot(right).atan2(flat.dot(fwd));
        STRAFE_CLIPS[((angle / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8) as usize]
    };
    let want: &'static str = match state {
        MoveState::Ground if throwing_grenade => "grenade_throw",
        MoveState::Ground if gun_aim && speed > 0.6 => strafe,
        MoveState::Ground if gun_aim => "aim_idle",
        MoveState::Ground if pistol_out && speed > 0.6 => "pistol_run",
        MoveState::Ground if pistol_out && rig.land_timer <= 0.0 => "pistol_idle",
        MoveState::Ground if rig.land_timer > 0.0 && speed > WALK_TO_RUN => "land_run",
        MoveState::Ground if rig.land_timer > 0.0 && speed > 0.6 => "land_walk",
        MoveState::Ground if speed > 0.6 && rig.walking => "walk",
        MoveState::Ground if speed > 0.6 => "run",
        MoveState::Ground if rig.land_timer > 0.0 => "land",
        MoveState::Ground if rig.idle_time > FIDGET_AFTER => FIDGETS[rig.fidgets_played % FIDGETS.len()],
        MoveState::Ground => "idle",
        MoveState::Jump if v.y < -5.0 => "fall",
        MoveState::Jump => "jump",
        MoveState::DoubleJump => "double_jump",
        MoveState::Fall => "fall",
        MoveState::Glide => "glide",
        MoveState::Dash => "dash",
        MoveState::AirDash => "air_dash",
        MoveState::Melee => MELEE_CLIPS[p.sim.melee_swing().unwrap_or(0).min(MELEE_CLIPS.len() - 1)],
        MoveState::Smash => "smash_intro",
        MoveState::SmashLand => "smash",
    };
    let want = if rig.clips.contains_key(want) {
        want
    } else if want == "walk" || want.starts_with("land_") {
        if speed > 0.6 { "run" } else { "land" }
    } else {
        "idle"
    };
    if rig.current_clip != Some(want)
        && let Some(&node) = rig.clips.get(want)
    {
        // Wrench moves, the grenade throw and strafe changes cut in quickly.
        let quick = |c: Option<&str>| c.is_some_and(|c| MELEE_CLIPS.contains(&c) || c.starts_with("smash") || c.starts_with("strafe") || c == "grenade_throw");
        let landing = |c: Option<&str>| c.is_some_and(|c| c.starts_with("land"));
        let fade = if quick(Some(want)) || quick(rig.current_clip) {
            60
        } else if landing(Some(want)) || landing(rig.current_clip) {
            90
        } else if FIDGETS.contains(&want) || rig.current_clip.is_some_and(|c| FIDGETS.contains(&c)) {
            400
        } else {
            160
        };
        let active = transitions.play(&mut player, node, Duration::from_millis(fade));
        if LOOPING.contains(&want) {
            active.repeat();
        }
        rig.current_clip = Some(want);
    }
    // Match the walk, run and strafe cycles (and the moving landings) to the actual speed: at
    // the speed a clip was authored for, it plays at 1x.
    if (["run", "walk", "pistol_run", "land_walk", "land_run"].contains(&want) || want.starts_with("strafe"))
        && let Some(&node) = rig.clips.get(want)
    {
        let tuning = p.sim.tuning();
        let authored = rig.authored_speed.get(want).copied().unwrap_or_else(|| {
            (if want.starts_with("strafe") { tuning.aim_speeds.run_max } else { tuning.ground_speeds.run_max }) / settings.scale
        });
        if let Some(active) = player.animation_mut(node) {
            let range = if want.starts_with("land") { (0.7, 1.3) } else { (0.5, 1.5) };
            active.set_speed((speed / authored.max(0.1)).clamp(range.0, range.1));
        }
    }
    rig.walking = if rig.walking { speed < WALK_TO_RUN } else { speed < RUN_TO_WALK };
    lean_into_turns(&mut rig, &mut player, p.sim.facing_yaw(), want == "run", time.delta_secs());
    // Fidgets play once, then it's back to the idle (and a while until the next one).
    rig.idle_time = if want == "idle" { rig.idle_time + time.delta_secs() } else { rig.idle_time };
    if FIDGETS.contains(&want)
        && let Some(&node) = rig.clips.get(want)
        && player.animation(node).is_none_or(|a| a.is_finished())
    {
        rig.idle_time = 0.0;
        rig.fidgets_played += 1;
    } else if !FIDGETS.contains(&want) && want != "idle" {
        rig.idle_time = 0.0;
    }
}

/// Blends the run's lean-left and lean-right clips in by how fast he turns, kept in step with
/// the run (same length, same time), and fades them out when he stops running.
fn lean_into_turns(rig: &mut Rig, player: &mut AnimationPlayer, yaw: f32, running: bool, dt: f32) {
    let Some(nodes) = rig.lean_nodes else { return };
    let rate = rig.prev_yaw.map_or(0.0, |prev| {
        let mut d = yaw - prev;
        while d > PI {
            d -= TAU;
        }
        while d < -PI {
            d += TAU;
        }
        d / dt.max(1e-4)
    });
    rig.prev_yaw = Some(yaw);
    // Turning toward +yaw is to his left.
    let target = if running { (rate / FULL_LEAN_TURN_RATE).clamp(-1.0, 1.0) } else { 0.0 };
    rig.lean += (target - rig.lean) * (1.0 - (-dt / 0.15).exp());
    rig.lean_blend += ((running as u8 as f32) - rig.lean_blend) * (1.0 - (-dt / 0.08).exp());
    let run = rig.clips.get("run").and_then(|&n| player.animation(n)).map(|a| (a.seek_time(), a.speed(), a.weight()));
    let weights = [rig.lean.max(0.0), (-rig.lean).max(0.0)];
    for (node, w) in nodes.into_iter().zip(weights) {
        let w = w * rig.lean_blend;
        match run {
            Some((time, speed, run_weight)) if w > 1e-3 => {
                if player.animation(node).is_none() {
                    player.start(node).repeat();
                }
                if let Some(active) = player.animation_mut(node) {
                    active.seek_to(time).set_speed(speed).set_weight(w * run_weight);
                }
            }
            _ => {
                player.stop(node);
            }
        }
    }
    // The run itself gives up what the lean takes.
    if running
        && let Some(&node) = rig.clips.get("run")
        && let Some(active) = player.animation_mut(node)
    {
        active.set_weight(1.0 - rig.lean.abs() * rig.lean_blend);
    }
}

/// Reads each clip's authored ground speed (root motion over its length) from the converter's
/// manifest.
fn authored_speeds(manifest: &std::path::Path) -> HashMap<String, f32> {
    let Ok(text) = std::fs::read_to_string(manifest) else { return HashMap::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return HashMap::new() };
    let Some(anims) = v["animations"].as_object() else { return HashMap::new() };
    anims
        .iter()
        .filter_map(|(name, a)| {
            let (d, t) = (a["root_distance"].as_f64()?, a["seconds"].as_f64()?);
            (d > 0.5 && t > 0.0).then(|| (name.clone(), (d / t) as f32))
        })
        .collect()
}

fn target_pose(state: MoveState, speed: f32, vy: f32, run_max: f32, phase: f32, t: f32) -> Pose {
    let s = (speed / run_max.max(0.1)).clamp(0.0, 1.3);
    let falling = (-vy / 10.0).clamp(0.0, 1.0);
    let sway = (t * 1.7).sin();
    match state {
        MoveState::Ground if speed > 0.25 => {
            let (a, b) = (phase.sin(), (phase + PI).sin());
            Pose {
                hip: [-0.65 * s * a, -0.65 * s * b],
                knee: [0.15 + 1.0 * s * (phase + FRAC_PI_2).sin().max(0.0), 0.15 + 1.0 * s * (phase + PI + FRAC_PI_2).sin().max(0.0)],
                arm_drop: 1.2,
                arm_swing: [0.7 * s * a, 0.7 * s * b],
                elbow: 0.4 + 0.6 * s,
                lean: 0.28 * s,
                head: -0.15 * s,
                ears: 0.35 * s,
                tail_lift: -0.25 * s,
                tail_sway: 0.35 * s * (phase * 0.5).sin(),
            }
        }
        MoveState::Ground => Pose {
            hip: [0.0; 2],
            knee: [0.08; 2],
            arm_drop: 1.25,
            arm_swing: [0.05, 0.05],
            elbow: 0.25,
            lean: 0.04 + 0.02 * (t * 2.0).sin(),
            head: 0.0,
            ears: 0.05 * (t * 0.9).sin(),
            tail_lift: 0.0,
            tail_sway: 0.25 * sway,
        },
        MoveState::Jump => Pose {
            hip: [-0.9, -0.3],
            knee: [1.3, 0.5],
            arm_drop: 0.7,
            arm_swing: [-0.5, -0.5],
            elbow: 0.8,
            lean: 0.1,
            head: -0.1,
            ears: 0.6 * falling,
            tail_lift: 0.3,
            tail_sway: 0.1 * sway,
        },
        MoveState::DoubleJump => Pose {
            hip: [-1.4, -1.2],
            knee: [2.0, 1.9],
            arm_drop: 0.9,
            arm_swing: [-0.9, -0.9],
            elbow: 1.3,
            lean: 0.45,
            head: 0.1,
            ears: 0.7,
            tail_lift: 0.6,
            tail_sway: 0.0,
        },
        MoveState::Fall => Pose {
            hip: [-0.3, 0.2],
            knee: [0.6, 0.3],
            arm_drop: 0.2,
            arm_swing: [-0.3, -0.2],
            elbow: 0.5,
            lean: -0.05,
            head: -0.2,
            ears: -0.6 * falling,
            tail_lift: 0.5,
            tail_sway: 0.3 * sway,
        },
        MoveState::Glide => Pose {
            hip: [0.45, 0.45],
            knee: [0.5, 0.5],
            arm_drop: -0.05,
            arm_swing: [0.25, 0.25],
            elbow: 0.1,
            lean: 0.35,
            head: -0.35,
            ears: -0.5,
            tail_lift: -0.2,
            tail_sway: 0.15 * sway,
        },
        MoveState::Dash | MoveState::AirDash | MoveState::Melee | MoveState::Smash | MoveState::SmashLand => Pose {
            hip: [-0.7, 0.8],
            knee: [0.9, 0.6],
            arm_drop: 1.1,
            arm_swing: [1.0, 1.0],
            elbow: 0.3,
            lean: 0.55,
            head: -0.45,
            ears: -0.8,
            tail_lift: -0.5,
            tail_sway: 0.0,
        },
    }
}

fn animate(
    time: Res<Time>,
    settings: Res<Settings>,
    rig: Option<ResMut<Rig>>,
    players: Query<&Player>,
    mut transforms: Query<&mut Transform>,
) {
    let (Some(mut rig), Ok(p)) = (rig, players.single()) else { return };
    if !rig.clips.is_empty() {
        return; // the game's own clips are playing
    }
    let dt = time.delta_secs();
    let v = p.sim.velocity() / settings.scale;
    let speed = Vec2::new(v.x, v.z).length();
    let run_max = p.sim.tuning().ground_speeds.run_max / settings.scale;
    // About 1.5 m per stride at Ratchet's size.
    rig.phase = (rig.phase + dt * speed / 1.5 * TAU) % (TAU * 2.0);
    let target = target_pose(p.sim.state(), speed, v.y, run_max, rig.phase, time.elapsed_secs());
    let k = 1.0 - (-14.0 * dt).exp();
    rig.pose = rig.pose.lerp(&target, k);
    let pose = rig.pose;
    let [rest_l, rest_r] = rig.arm_rest_angle;

    let mut set = |name: &str, r: Quat| {
        if let Some(b) = rig.bones.get(name)
            && let Ok(mut t) = transforms.get_mut(b.entity)
        {
            t.rotation = b.parent_rest.inverse() * r * b.parent_rest * b.rest_local;
        }
    };
    let x = Quat::from_rotation_x;
    let y = Quat::from_rotation_y;
    let z = Quat::from_rotation_z;

    for (i, side) in ["LF", "RT"].into_iter().enumerate() {
        set(&format!("{side}_upleg"), x(pose.hip[i]));
        set(&format!("{side}_loleg"), x(pose.knee[i]));
        // Left arm points +X, right arm −X. Drop angle below horizontal, then swing.
        let target_angle = if i == 0 { -pose.arm_drop } else { pose.arm_drop - PI };
        let rest = if i == 0 { rest_l } else { rest_r };
        let mut drop = target_angle - rest;
        while drop > PI {
            drop -= TAU;
        }
        while drop < -PI {
            drop += TAU;
        }
        set(&format!("{side}_uparm"), x(-pose.arm_swing[i]) * z(drop));
        set(&format!("{side}_loarm"), y(if i == 0 { -pose.elbow } else { pose.elbow }));
        set(&format!("{side}_ear_1"), x(-pose.ears));
    }
    for s in ["spine_1", "spine_2", "spine_3"] {
        set(s, x(pose.lean / 3.0));
    }
    set("neck_01", x(pose.head * 0.5));
    set("head", x(pose.head * 0.5));
    for (i, s) in ["tail_1", "tail_2", "tail_3", "tail_4", "tail_5", "tail_6"].into_iter().enumerate() {
        let f = 1.0 + i as f32 * 0.3;
        set(s, y(pose.tail_sway * 0.25 * f) * x(pose.tail_lift * 0.15));
    }
}
