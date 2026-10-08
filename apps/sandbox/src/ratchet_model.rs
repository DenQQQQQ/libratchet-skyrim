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
use libratchet::{LedgeMove, MoveState};
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
    "aim_idle", "strafe_n", "strafe_ne", "strafe_e", "strafe_se", "strafe_s", "strafe_sw", "strafe_w", "strafe_nw",
    "pistol_idle", "pistol_run", "walk", "land_walk", "land_run", "idle_look", "idle_shoe", "glove_idle", "wrench_aim_idle",
    "wrench_strafe_n", "wrench_strafe_ne", "wrench_strafe_e", "wrench_strafe_se", "wrench_strafe_s", "wrench_strafe_sw", "wrench_strafe_w",
    "wrench_strafe_nw", "strafe_walk_n", "strafe_walk_ne", "strafe_walk_e", "strafe_walk_se", "strafe_walk_s", "strafe_walk_sw", "strafe_walk_w", "strafe_walk_nw",
    "wrench_strafe_walk_n", "wrench_strafe_walk_ne", "wrench_strafe_walk_e", "wrench_strafe_walk_se", "wrench_strafe_walk_s", "wrench_strafe_walk_sw", "wrench_strafe_walk_w", "wrench_strafe_walk_nw",
    "wrench_idle", "wrench_walk", "wrench_run", "wrench_jump", "wrench_double_jump", "wrench_land", "wrench_land_walk", "wrench_land_run",
    "wrench_glide", "glove_glide", "dash_ne", "dash_e", "dash_se", "dash_s", "dash_sw", "dash_w", "dash_nw", "air_dash_ne", "air_dash_e",
    "air_dash_se", "air_dash_s", "air_dash_sw", "air_dash_w", "air_dash_nw", "glide_intro_pistol", "glide_pistol", "glide_intro_wrench",
    "glide_intro_glove", "strafe_jump_n", "strafe_jump_e", "strafe_jump_s", "strafe_jump_w", "strafe_jump_idle", "wrench_strafe_jump_n",
    "wrench_strafe_jump_e", "wrench_strafe_jump_s", "wrench_strafe_jump_w", "strafe_double_jump_n", "strafe_double_jump_e",
    "strafe_double_jump_s", "strafe_double_jump_w", "wrench_strafe_double_jump_n", "wrench_strafe_double_jump_e",
    "wrench_strafe_double_jump_s", "wrench_strafe_double_jump_w", "strafe_land_n", "strafe_land_e", "strafe_land_s", "strafe_land_w",
    "strafe_land_idle", "wrench_strafe_land_n", "wrench_strafe_land_e", "wrench_strafe_land_s", "wrench_strafe_land_w",
    "wrench_strafe_land_idle", "wall_stick_left", "wall_stick_right", "wall_slide_left", "wall_slide_right", "wall_jump_left",
    "wall_jump_right", "ledge_grab", "ledge_from_fall", "ledge_from_fall_l180", "ledge_from_fall_r180", "ledge_idle", "ledge_up",
    "ledge_down_to", "ledge_jump", "ledge_drop", "ledge_drop_turn", "swing_pull", "swing_intro", "swing_forward_fast",
    "swing_forward_slow", "swing_backward_fast", "swing_backward_slow", "swing_jump", "hook_reel",
];
/// Dashes by direction relative to where he faces (clockwise from ahead), on the ground and in
/// the air: the game's Dash and Air_Dash drivers have one clip per direction.
const DASH_CLIPS: [&str; 8] = ["dash", "dash_ne", "dash_e", "dash_se", "dash_s", "dash_sw", "dash_w", "dash_nw"];
const AIR_DASH_CLIPS: [&str; 8] = ["air_dash", "air_dash_ne", "air_dash_e", "air_dash_se", "air_dash_s", "air_dash_sw", "air_dash_w", "air_dash_nw"];
/// The runs' lean-into-a-turn versions (left, right), blended in step with them by how fast he
/// turns: empty-handed and with the wrench.
const LEAN_CLIPS: [(&str, [&str; 2]); 2] = [("run", ["run_lean_left", "run_lean_right"]), ("wrench_run", ["wrench_run_lean_left", "wrench_run_lean_right"])];
/// The glove's hand pose (fingers, wrist and grip), over whatever he does while the glove is on.
/// It's a pose of those joints only, weighted far over the move clips under it (ours).
const GLOVE_HAND: &str = "glove_hand";
const GLOVE_HAND_WEIGHT: f32 = 20.0;
/// Drawing a weapon (additive, the game's Weapon_Draw driver): for the one-handed gun, the
/// wrench and the glove, each whole-body (standing) and upper-body (moving).
const DRAW_LAYERS: [[&str; 2]; 3] = [
    ["draw_pistol_additive", "draw_pistol_arm_additive"],
    ["draw_wrench_additive", "draw_wrench_arm_additive"],
    ["draw_glove_additive", "draw_glove_arm_additive"],
];
/// The draw fades in and out over these (the driver's blend times).
const DRAW_FADE_IN: f32 = 0.15;
const DRAW_FADE_OUT: f32 = 0.25;
/// Jumping while strafing (aiming or firing): by the jump's direction relative to where he faces
/// (n ahead, e his right, s back, w his left; the sideways and backward ones flip), then
/// standing still, for the one-handed grip and the wrench/glove grip. Double jumps have no
/// standing version (ahead), and landings their own set.
const STRAFE_JUMPS: [[&str; 5]; 2] = [
    ["strafe_jump_n", "strafe_jump_e", "strafe_jump_s", "strafe_jump_w", "strafe_jump_idle"],
    ["wrench_strafe_jump_n", "wrench_strafe_jump_e", "wrench_strafe_jump_s", "wrench_strafe_jump_w", "strafe_jump_idle"],
];
const STRAFE_DOUBLE_JUMPS: [[&str; 5]; 2] = [
    ["strafe_double_jump_n", "strafe_double_jump_e", "strafe_double_jump_s", "strafe_double_jump_w", "strafe_double_jump_n"],
    ["wrench_strafe_double_jump_n", "wrench_strafe_double_jump_e", "wrench_strafe_double_jump_s", "wrench_strafe_double_jump_w", "wrench_strafe_double_jump_n"],
];
const STRAFE_LANDINGS: [[&str; 5]; 2] = [
    ["strafe_land_n", "strafe_land_e", "strafe_land_s", "strafe_land_w", "strafe_land_idle"],
    ["wrench_strafe_land_n", "wrench_strafe_land_e", "wrench_strafe_land_s", "wrench_strafe_land_w", "wrench_strafe_land_idle"],
];
/// The glide's lean (additive, 11 poses, the middle one upright), scrubbed by how fast he turns,
/// fully at FULL_LEAN_TURN_RATE.
const GLIDE_LEAN: &str = "glide_lean_additive";
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
/// The game's grenade throws (underhand, additive), layered over whatever Ratchet does: the
/// whole-body one standing still, the upper-body one on the move or in the air.
const GRENADE_LAYERS: [&str; 2] = ["grenade_throw_additive", "grenade_throw_arm_additive"];
/// Winding a grenade throw up while the fire button is held (the same two versions).
const WINDUP_LAYERS: [&str; 2] = ["grenade_windup_additive", "grenade_windup_arm_additive"];
/// The aim offsets (additive, 21 poses scrubbed by the aim angle): one-handed and wrench/glove
/// grip, standing and running, each up/down (v) and left/right (h).
const AIM_LAYERS: [&str; 8] = ["aim_pistol_v_additive", "aim_pistol_h_additive", "aim_pistol_run_v_additive", "aim_pistol_run_h_additive", "aim_wrench_v_additive", "aim_wrench_h_additive", "aim_wrench_run_v_additive", "aim_wrench_run_h_additive"];
/// Aim angles at the first and last pose (degrees). The middle pose aims straight ahead. Measured
/// from the pistol's poses (its arm reaches +87°/-45° and -64°/+96°); the game's own
/// parameter ranges weren't decoded, and the wrench/glove poses are assumed to match.
const AIM_UP: f32 = 90.0;
const AIM_DOWN: f32 = 45.0;
const AIM_SIDE: f32 = 90.0;
/// Strafing while aiming, by direction relative to where Ratchet faces (clockwise from ahead).
const STRAFE_CLIPS: &[&str] = &["strafe_n", "strafe_ne", "strafe_e", "strafe_se", "strafe_s", "strafe_sw", "strafe_w", "strafe_nw"];
/// The walking versions (aim mode's pace).
const STRAFE_WALK_CLIPS: &[&str] = &["strafe_walk_n", "strafe_walk_ne", "strafe_walk_e", "strafe_walk_se", "strafe_walk_s", "strafe_walk_sw", "strafe_walk_w", "strafe_walk_nw"];
const WRENCH_STRAFE_WALK_CLIPS: &[&str] = &["wrench_strafe_walk_n", "wrench_strafe_walk_ne", "wrench_strafe_walk_e", "wrench_strafe_walk_se", "wrench_strafe_walk_s", "wrench_strafe_walk_sw", "wrench_strafe_walk_w", "wrench_strafe_walk_nw"];
/// The same with the wrench or the glove (grenades) in hand.
const WRENCH_STRAFE_CLIPS: &[&str] = &[
    "wrench_strafe_n", "wrench_strafe_ne", "wrench_strafe_e", "wrench_strafe_se", "wrench_strafe_s", "wrench_strafe_sw", "wrench_strafe_w",
    "wrench_strafe_nw",
];
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
/// Swinging faster than this (m/s, at Ratchet's size) plays the fast swing clips (ours).
const SWING_FAST: f32 = 6.0;
/// The wall jump's in-air clip plays at this rate (see `play_clips`).
const WALL_JUMP_CLIP_RATE: f32 = 0.25;
const LOOPING: &[&str] = &[
    "idle", "run", "fall", "glide", "aim_idle", "strafe_n", "strafe_ne", "strafe_e", "strafe_se", "strafe_s", "strafe_sw", "strafe_w", "strafe_nw",
    "pistol_idle", "pistol_run", "walk", "glove_idle", "wrench_aim_idle", "wrench_strafe_n", "wrench_strafe_ne", "wrench_strafe_e",
    "wrench_strafe_se", "wrench_strafe_s", "wrench_strafe_sw", "wrench_strafe_w", "wrench_strafe_nw", "strafe_walk_n", "strafe_walk_ne", "strafe_walk_e", "strafe_walk_se", "strafe_walk_s", "strafe_walk_sw", "strafe_walk_w", "strafe_walk_nw",
    "wrench_strafe_walk_n", "wrench_strafe_walk_ne", "wrench_strafe_walk_e", "wrench_strafe_walk_se", "wrench_strafe_walk_s", "wrench_strafe_walk_sw", "wrench_strafe_walk_w", "wrench_strafe_walk_nw",
    "wrench_idle", "wrench_walk", "wrench_run", "wrench_glide", "glove_glide", "glide_pistol", "wall_slide_left", "wall_slide_right",
    "ledge_idle", "swing_forward_fast", "swing_forward_slow", "swing_backward_fast", "swing_backward_slow", "hook_reel",
];
/// Played at the rate that keeps the feet planted.
const SPEED_MATCHED: &[&str] = &["run", "walk", "pistol_run", "land_walk", "land_run", "wrench_run", "wrench_walk", "wrench_land_walk", "wrench_land_run"];

/// With the glove on, or the melee grip during swings and throws (the game's thrown and melee
/// grips), the moves that have wrench-carrying versions use them, as the game's animset picks.
fn carried(clip: &'static str) -> &'static str {
    match clip {
        "idle" => "wrench_idle",
        "walk" => "wrench_walk",
        "run" => "wrench_run",
        "jump" => "wrench_jump",
        "double_jump" => "wrench_double_jump",
        "land" => "wrench_land",
        "land_walk" => "wrench_land_walk",
        "land_run" => "wrench_land_run",
        other => other,
    }
}

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
    /// The player's let-go and respawn counts this has seen.
    drops_seen: u32,
    respawns_seen: u32,
    /// The speed each clip was authored for (its root motion over its length, from the
    /// converter's ratchet.json), to play it at the rate that keeps the feet planted.
    authored_speed: HashMap<String, f32>,
    /// The sounds each clip calls for (Wwise event names, and "footstep" at its steps) at their
    /// times, and the clip and time they were last checked at.
    clip_sounds: HashMap<String, Vec<(f32, String)>>,
    sound_clock: Option<(&'static str, f32)>,
    walking: bool,
    /// The run's lean: −1 (right) … 1 (left), each run's two lean clips, and the yaw it was last
    /// measured at.
    lean: f32,
    lean_nodes: Vec<(&'static str, [AnimationNodeIndex; 2])>,
    lean_blend: f32,
    prev_yaw: Option<f32>,
    /// The wrench catch (upper body), how long it has played, and the catch count it belongs to.
    catch_layer: Option<(AnimationNodeIndex, f32)>,
    catch_time: Option<f32>,
    catches_seen: u32,
    /// The grenade throw layers (see GRENADE_LAYERS), the one playing and for how long, and the
    /// throw count they've caught up with.
    grenade_layers: [Option<(AnimationNodeIndex, f32)>; 2],
    grenade_time: Option<(usize, f32)>,
    grenades_seen: u32,
    /// The windup layers, and the one playing with its weight.
    windup_layers: [Option<AnimationNodeIndex>; 2],
    windup: Option<(usize, f32)>,
    /// The aim-offset layers (see AIM_LAYERS) and how far they're faded in.
    aim_layers: [Option<(AnimationNodeIndex, f32)>; 8],
    aim_weight: f32,
    /// The dash clip picked when the dash started (its direction relative to his facing).
    dash_clip: Option<&'static str>,
    /// The glove's hand pose and how far it's faded in.
    glove_hand: Option<AnimationNodeIndex>,
    glove_weight: f32,
    /// The draw layers (see DRAW_LAYERS), the one playing (grip, which, time), the weapon
    /// switches seen, and a draw waiting for a swing to end.
    draw_layers: [[Option<(AnimationNodeIndex, f32)>; 2]; 3],
    draw: Option<(usize, usize, f32)>,
    equips_seen: u32,
    draw_pending: bool,
    /// The strafe jump picked as the jump (or double jump) started, and the strafe landing.
    jump_clip: Option<(MoveState, Option<&'static str>)>,
    strafe_landing: Option<&'static str>,
    /// Gliding: how long, whether the intro is over, and the lean layer with its weight and
    /// value (−1 right … 1 left).
    glide_time: f32,
    glide_looping: bool,
    glide_lean: Option<(AnimationNodeIndex, f32)>,
    glide_lean_weight: f32,
    glide_lean_value: f32,
    /// How fast he's turning (rad/s, + to his left).
    turn_rate: f32,
    /// On a wall: whether the stick clip is over (then the slide loop plays).
    wall_looping: bool,
    /// Catching a ledge: the catch clip for how far he turns to face it; and the time left of
    /// the let-go clip after dropping off a caught ledge.
    ledge_catch: &'static str,
    ledge_drop_time: f32,
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

#[allow(clippy::too_many_arguments)]
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
    // The Shatterbomb's glove goes on over his right glove (sm05), skinned to his joints.
    if let Some(dir) = settings.model_dir.as_ref()
        && dir.join(crate::weapons::GLOVE_FILE).exists()
    {
        crate::weapons::spawn_glove(&mut commands, &assets, root, by_name.clone(), by_name.get("sm05").copied());
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
    let clip_sounds = settings.model_dir.as_ref().map(|d| clip_sounds(&d.join("ratchet.json"), &d.join("wrench.json"))).unwrap_or_default();
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
        wall_looping: false,
        ledge_catch: "ledge_from_fall",
        ledge_drop_time: 0.0,
        land_timer: 0.0,
        throw_layer: None,
        throw_time: None,
        throws_seen: 0,
        fire_layer: None,
        fire_time: None,
        shots_seen: 0,
        drops_seen: 0,
        respawns_seen: 0,
        authored_speed,
        clip_sounds,
        sound_clock: None,
        walking: false,
        lean: 0.0,
        lean_nodes: Vec::new(),
        lean_blend: 0.0,
        prev_yaw: None,
        catch_layer: None,
        catch_time: None,
        catches_seen: 0,
        grenade_layers: [None, None],
        grenade_time: None,
        windup_layers: [None, None],
        windup: None,
        aim_layers: [None; 8],
        aim_weight: 0.0,
        grenades_seen: 0,
        glove_hand: None,
        glove_weight: 0.0,
        draw_layers: [[None; 2]; 3],
        draw: None,
        equips_seen: 0,
        draw_pending: false,
        jump_clip: None,
        strafe_landing: None,
        glide_time: 0.0,
        glide_looping: false,
        glide_lean: None,
        glide_lean_weight: 0.0,
        glide_lean_value: 0.0,
        turn_rate: 0.0,
        dash_clip: None,
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
    for (run, names) in LEAN_CLIPS {
        if let [Some(left), Some(right)] = names.map(|n| gltf.named_animations.get(n)) {
            rig.lean_nodes.push((run, [graph.add_clip(left.clone(), 1.0, base), graph.add_clip(right.clone(), 1.0, base)]));
        }
    }
    for (grip, names) in DRAW_LAYERS.iter().enumerate() {
        for (which, name) in names.iter().enumerate() {
            if let Some(clip) = gltf.named_animations.get(*name) {
                let duration = clip_assets.get(clip).map_or(0.93, |c| c.duration());
                rig.draw_layers[grip][which] = Some((graph.add_clip(clip.clone(), 1.0, layers), duration));
            }
        }
    }
    if let Some(clip) = gltf.named_animations.get(GLIDE_LEAN) {
        let duration = clip_assets.get(clip).map_or(0.33, |c| c.duration());
        rig.glide_lean = Some((graph.add_clip(clip.clone(), 1.0, layers), duration));
    }
    if let Some(clip) = gltf.named_animations.get(GLOVE_HAND) {
        rig.glove_hand = Some(graph.add_clip(clip.clone(), 1.0, base));
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
    for (i, name) in GRENADE_LAYERS.iter().enumerate() {
        if let Some(clip) = gltf.named_animations.get(*name) {
            let duration = clip_assets.get(clip).map_or(0.5, |c| c.duration());
            rig.grenade_layers[i] = Some((graph.add_clip(clip.clone(), 1.0, layers), duration));
        }
    }
    for (i, name) in WINDUP_LAYERS.iter().enumerate() {
        if let Some(clip) = gltf.named_animations.get(*name) {
            rig.windup_layers[i] = Some(graph.add_clip(clip.clone(), 1.0, layers));
        }
    }
    for (i, name) in AIM_LAYERS.iter().enumerate() {
        if let Some(clip) = gltf.named_animations.get(*name) {
            let duration = clip_assets.get(clip).map_or(0.67, |c| c.duration());
            rig.aim_layers[i] = Some((graph.add_clip(clip.clone(), 1.0, layers), duration));
        }
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
    mut cues: MessageWriter<crate::sound::SoundCue>,
) {
    let (Some(mut rig), Ok(p)) = (rig, players.single()) else { return };
    // Respawned: no landing or let-go clip from where he was before.
    if p.respawns != rig.respawns_seen {
        rig.respawns_seen = p.respawns;
        rig.prev_state = None;
        rig.land_timer = 0.0;
        rig.ledge_drop_time = 0.0;
    }
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

    // A grenade throw: the whole-body one standing still, the upper-body one on the move or in
    // the air, layered over the move (weapons.rs times the release by the same rule).
    let flat_speed = Vec2::new(p.sim.velocity().x, p.sim.velocity().z).length() / settings.scale;
    let standing = p.sim.state() == MoveState::Ground && flat_speed <= 0.6;
    let throws = arsenal.as_ref().map_or(0, |a| a.throws);
    if throws != rig.grenades_seen {
        rig.grenades_seen = throws;
        let which = if standing { 0 } else { 1 };
        if let Some((node, _)) = rig.grenade_layers[which] {
            if let Some((prev, _)) = rig.grenade_time.and_then(|(i, _)| rig.grenade_layers[i]) {
                player.stop(prev);
            }
            player.start(node);
            rig.grenade_time = Some((which, 0.0));
        }
    }
    if let Some((which, t)) = rig.grenade_time
        && let Some((node, duration)) = rig.grenade_layers[which]
    {
        let t = t + time.delta_secs();
        if t >= duration {
            player.stop(node);
            rig.grenade_time = None;
        } else {
            let weight = (t / LAYER_FADE_IN).min(1.0).min(((duration - t) / LAYER_FADE_OUT).max(0.0));
            if let Some(active) = player.animation_mut(node) {
                active.set_weight(weight);
            }
            rig.grenade_time = Some((which, t));
        }
    }

    // Winding a grenade throw up while the fire button is held: the windup plays once and holds
    // its last pose; the throw takes over from it.
    let cooking = arsenal.as_ref().is_some_and(|a| a.cooking);
    let throwing = rig.grenade_time.is_some_and(|(_, t)| t < 0.05);
    match rig.windup {
        None if cooking && !throwing => {
            let which = if standing { 0 } else { 1 };
            if let Some(node) = rig.windup_layers[which] {
                player.start(node).set_weight(0.0);
                rig.windup = Some((which, 0.0));
            }
        }
        Some((which, weight)) => {
            let on = cooking && !throwing;
            let weight = if on { (weight + time.delta_secs() / LAYER_FADE_IN).min(1.0) } else if throwing { 0.0 } else { weight - time.delta_secs() / 0.12 };
            if let Some(node) = rig.windup_layers[which] {
                if weight <= 0.0 {
                    player.stop(node);
                    rig.windup = None;
                } else {
                    if let Some(active) = player.animation_mut(node) {
                        active.set_weight(weight);
                    }
                    rig.windup = Some((which, weight));
                }
            }
        }
        None => {}
    }

    // Aim offsets while strafing (aiming, or a gun firing): the upper body bends toward what the
    // crosshair points at, by scrubbing the poses with the angle up/down and to the side of his
    // facing.
    // (Not while dashing: the dash clips are whole-body and the game puts the weapon away.)
    let throwing = wrench.throwing();
    let aiming_now = arsenal.as_ref().is_some_and(|a| a.aiming || (!throwing && (a.trigger || a.stance > 0.0)))
        && !matches!(p.sim.state(), MoveState::Dash | MoveState::AirDash);
    let fade = time.delta_secs() / LAYER_FADE_IN;
    rig.aim_weight = if aiming_now { (rig.aim_weight + fade).min(1.0) } else { (rig.aim_weight - fade).max(0.0) };
    let (pitch, side) = arsenal.as_ref().map_or((0.0, 0.0), |a| {
        let d = a.aim_target - (p.feet + Vec3::Y * 0.85 * settings.scale);
        let yaw = d.x.atan2(d.z) - p.sim.facing_yaw();
        let side = (yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        (d.y.atan2(Vec2::new(d.x, d.z).length()).to_degrees(), side.to_degrees())
    });
    let pistol_grip = !throwing && arsenal.as_ref().is_some_and(|a| a.slot == crate::weapons::Slot::BurstPistol);
    let running = flat_speed > 0.6;
    let active = (if pistol_grip { 0 } else { 4 }) + if running { 2 } else { 0 };
    for (i, layer) in rig.aim_layers.iter().enumerate() {
        let Some((node, duration)) = *layer else { continue };
        let weight = if i == active || i == active + 1 { rig.aim_weight } else { 0.0 };
        if weight <= 0.0 {
            if player.animation(node).is_some() {
                player.stop(node);
            }
            continue;
        }
        let half = duration / 2.0;
        let t = if i % 2 == 0 {
            if pitch >= 0.0 { half * (1.0 - pitch / AIM_UP) } else { half * (1.0 - pitch / AIM_DOWN) }
        } else {
            half * (1.0 + side / AIM_SIDE)
        };
        let anim = if player.animation(node).is_some() { player.animation_mut(node) } else { Some(player.start(node).repeat()) };
        if let Some(anim) = anim {
            anim.set_speed(0.0).seek_to(t.clamp(0.0, duration * 0.999)).set_weight(weight);
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
    let airborne = |s: Option<MoveState>| {
        matches!(
            s,
            Some(MoveState::Jump | MoveState::DoubleJump | MoveState::Fall | MoveState::Glide | MoveState::AirDash | MoveState::WallJump | MoveState::LedgeJump)
        )
    };
    let landed = state == MoveState::Ground && airborne(rig.prev_state);
    // The game puts the weapon away on walls and ledges and draws it again after (not between
    // one of their moves and the next).
    let on_wall = |s: Option<MoveState>| s.is_some_and(crate::weapons::holstered);
    if on_wall(rig.prev_state) && !on_wall(Some(state)) {
        rig.draw_pending = true;
    }
    // Catching a ledge: the catch clip turning round to it when he faced away (more than 135°;
    // the game blends these by the turn, ours picks one).
    if state == MoveState::LedgeHang && rig.prev_state != Some(MoveState::LedgeHang) && p.sim.ledge_move() == Some(LedgeMove::FromFall) {
        let turn = rig.prev_yaw.map_or(0.0, |prev| {
            let d = (p.sim.facing_yaw() - prev).rem_euclid(std::f32::consts::TAU);
            if d > std::f32::consts::PI { d - std::f32::consts::TAU } else { d }
        });
        rig.ledge_catch = if turn > 2.36 {
            "ledge_from_fall_l180"
        } else if turn < -2.36 {
            "ledge_from_fall_r180"
        } else {
            "ledge_from_fall"
        };
    }
    // Letting go of a caught ledge plays its let-go clip as he starts to fall (counted, since a
    // frame can run two ticks and the event lasts one).
    let dropped = p.drops != rig.drops_seen;
    rig.drops_seen = p.drops;
    rig.ledge_drop_time = if dropped && state == MoveState::Fall {
        0.5
    } else if state == MoveState::Fall {
        (rig.ledge_drop_time - time.delta_secs()).max(0.0)
    } else {
        0.0
    };
    if landed {
        rig.land_timer = if speed > 0.6 { 0.35 } else { 0.3 };
    }
    rig.prev_state = Some(state);
    rig.land_timer = (rig.land_timer - time.delta_secs()).max(0.0);

    // Aiming (any weapon), or firing a gun and just after: strafing by direction, with the
    // pistol's one-handed set, or the wrench/glove set (standing with the glove, its own idle).
    // Swinging, and from a wrench throw until the catch, it's the melee grip whatever weapon is
    // out (a throw is the melee grip's "fire" in the game's animset).
    let melee_grip = throwing || matches!(state, MoveState::Melee | MoveState::Smash | MoveState::SmashLand);
    let pistol_out = !melee_grip && arsenal.as_ref().is_some_and(|a| a.slot == crate::weapons::Slot::BurstPistol);
    let glove_out = !melee_grip && arsenal.as_ref().is_some_and(|a| a.slot == crate::weapons::Slot::Shatterbomb);
    let carrying = arsenal.is_some() && !pistol_out;
    let gun_aim = arsenal.as_ref().is_some_and(|a| a.aiming || (!throwing && (a.trigger || a.stance > 0.0)));
    let (strafe_clips, aim_idle) = match (pistol_out, rig.walking) {
        (true, false) => (STRAFE_CLIPS, "aim_idle"),
        (true, true) => (STRAFE_WALK_CLIPS, "aim_idle"),
        (false, walking) => (if walking { WRENCH_STRAFE_WALK_CLIPS } else { WRENCH_STRAFE_CLIPS }, if glove_out { "glove_idle" } else { "wrench_aim_idle" }),
    };

    // Switching weapons draws the new one: the game's draw for its grip, whole-body standing,
    // upper-body moving. A switch during a swing draws once the swing is over.
    let equips = arsenal.as_ref().map_or(0, |a| a.equips);
    if equips != rig.equips_seen {
        rig.equips_seen = equips;
        rig.draw_pending = true;
    }
    if rig.draw_pending && !melee_grip {
        rig.draw_pending = false;
        let grip = if pistol_out { 0 } else if glove_out { 2 } else { 1 };
        if let Some((node, _)) = rig.draw_layers[grip][if standing { 0 } else { 1 }] {
            if let Some((prev, _)) = rig.draw.and_then(|(g, w, _)| rig.draw_layers[g][w]) {
                player.stop(prev);
            }
            player.start(node).set_weight(0.0);
            rig.draw = Some((grip, if standing { 0 } else { 1 }, 0.0));
        }
    }
    if let Some((g, w, t)) = rig.draw
        && let Some((node, duration)) = rig.draw_layers[g][w]
    {
        let t = t + time.delta_secs();
        if t >= duration {
            player.stop(node);
            rig.draw = None;
        } else {
            if let Some(active) = player.animation_mut(node) {
                active.set_weight((t / DRAW_FADE_IN).min(1.0).min(((duration - t) / DRAW_FADE_OUT).max(0.0)));
            }
            rig.draw = Some((g, w, t));
        }
    }

    // The glove's hand pose while it's on.
    if let Some(node) = rig.glove_hand {
        let fade = time.delta_secs() / 0.15;
        rig.glove_weight = if glove_out { (rig.glove_weight + fade).min(1.0) } else { (rig.glove_weight - fade).max(0.0) };
        if rig.glove_weight > 0.0 {
            let anim = if player.animation(node).is_some() { player.animation_mut(node) } else { Some(player.start(node).repeat()) };
            if let Some(anim) = anim {
                anim.set_weight(GLOVE_HAND_WEIGHT * rig.glove_weight);
            }
        } else if player.animation(node).is_some() {
            player.stop(node);
        }
    }
    // Which of eight directions (clockwise from ahead) a ground-plane direction is, relative to
    // where he faces.
    let octant = |d: Vec2| {
        let yaw = p.sim.facing_yaw();
        let (fwd, right) = (Vec2::new(yaw.sin(), yaw.cos()), Vec2::new(-yaw.cos(), yaw.sin()));
        let angle = d.dot(right).atan2(d.dot(fwd));
        ((angle / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8) as usize
    };
    let strafe = strafe_clips[octant(Vec2::new(v.x, v.z))];
    // A dash keeps the clip for the direction it started in.
    if !matches!(state, MoveState::Dash | MoveState::AirDash) {
        rig.dash_clip = None;
    } else if rig.dash_clip.is_none() {
        let clips = if state == MoveState::Dash { DASH_CLIPS } else { AIR_DASH_CLIPS };
        let clip = clips[octant(p.sim.dash_dir())];
        rig.dash_clip = Some(if rig.clips.contains_key(clip) { clip } else { clips[0] });
    }
    // Jumping while strafing: the clip for the jump's direction relative to his facing, picked as
    // each jump starts (the game's Strafe_Jump drivers), and the landing for how he comes down.
    let grip_set = if pistol_out { 0 } else { 1 };
    let quarter = |d: Vec2| -> usize {
        if d.length() < 0.6 {
            return 4;
        }
        let yaw = p.sim.facing_yaw();
        let (fwd, right) = (Vec2::new(yaw.sin(), yaw.cos()), Vec2::new(-yaw.cos(), yaw.sin()));
        ((d.dot(right).atan2(d.dot(fwd)) / std::f32::consts::FRAC_PI_2).round() as i32).rem_euclid(4) as usize
    };
    let flat = Vec2::new(v.x, v.z);
    if matches!(state, MoveState::Jump | MoveState::DoubleJump) {
        if rig.jump_clip.is_none_or(|(s, _)| s != state) {
            let set = if state == MoveState::Jump { STRAFE_JUMPS } else { STRAFE_DOUBLE_JUMPS };
            let clip = Some(set[grip_set][quarter(flat)]).filter(|c| gun_aim && rig.clips.contains_key(c));
            rig.jump_clip = Some((state, clip));
        }
    } else {
        rig.jump_clip = None;
    }
    if landed {
        rig.strafe_landing = Some(STRAFE_LANDINGS[grip_set][quarter(flat)]).filter(|c| gun_aim && !rig.walking && rig.clips.contains_key(c));
    } else if rig.land_timer <= 0.0 {
        rig.strafe_landing = None;
    }
    let strafe_jump = rig.jump_clip.and_then(|(_, c)| c);
    // Gliding under the heli-pack: the intro for his grip, then its loop.
    let (glide_intro, glide_loop) = if pistol_out {
        ("glide_intro_pistol", "glide_pistol")
    } else if glove_out {
        ("glide_intro_glove", "glove_glide")
    } else {
        ("glide_intro_wrench", "wrench_glide")
    };
    if state == MoveState::Glide {
        rig.glide_time += time.delta_secs();
        if rig.current_clip == Some(glide_intro) && rig.clips.get(glide_intro).and_then(|&n| player.animation(n)).is_some_and(|a| a.is_finished()) {
            rig.glide_looping = true;
        }
    } else {
        rig.glide_time = 0.0;
        rig.glide_looping = false;
    }
    let heli_glide = rig.clips.contains_key(glide_intro) && rig.clips.contains_key(glide_loop);
    // On a wall: onto it (the stick clip), then the slide loop once that's over, still or
    // sliding, as the game does. Left or right is the sim's pick (by the camera, as in the game).
    let (wall_stick, wall_slide, wall_jump) = if p.sim.wall_right() {
        ("wall_stick_right", "wall_slide_right", "wall_jump_right")
    } else {
        ("wall_stick_left", "wall_slide_left", "wall_jump_left")
    };
    if state != MoveState::WallStick {
        rig.wall_looping = false;
    } else if rig.current_clip == Some(wall_stick) && rig.clips.get(wall_stick).and_then(|&n| player.animation(n)).is_some_and(|a| a.is_finished()) {
        rig.wall_looping = true;
    }
    let want: &'static str = match state {
        MoveState::Ground if rig.land_timer > 0.0 && rig.strafe_landing.is_some() => rig.strafe_landing.unwrap_or("land"),
        MoveState::Ground if gun_aim && speed > 0.6 => strafe,
        MoveState::Ground if gun_aim => aim_idle,
        MoveState::Ground if pistol_out && speed > 0.6 => "pistol_run",
        MoveState::Ground if pistol_out && rig.land_timer <= 0.0 => "pistol_idle",
        MoveState::Ground if rig.land_timer > 0.0 && speed > WALK_TO_RUN => "land_run",
        MoveState::Ground if rig.land_timer > 0.0 && speed > 0.6 => "land_walk",
        MoveState::Ground if speed > 0.6 && rig.walking => "walk",
        MoveState::Ground if speed > 0.6 => "run",
        MoveState::Ground if rig.land_timer > 0.0 => "land",
        MoveState::Ground if rig.idle_time > FIDGET_AFTER => FIDGETS[rig.fidgets_played % FIDGETS.len()],
        MoveState::Ground => "idle",
        MoveState::Jump | MoveState::DoubleJump if strafe_jump.is_some() => strafe_jump.unwrap_or("jump"),
        MoveState::Jump if v.y < -5.0 => "fall",
        MoveState::Jump => "jump",
        MoveState::DoubleJump => "double_jump",
        MoveState::Fall if rig.ledge_drop_time > 0.0 => "ledge_drop",
        MoveState::Fall => "fall",
        MoveState::Glide if heli_glide && rig.glide_looping => glide_loop,
        MoveState::Glide if heli_glide => glide_intro,
        MoveState::Glide => "glide",
        MoveState::Dash | MoveState::AirDash => rig.dash_clip.unwrap_or("dash"),
        MoveState::Melee => MELEE_CLIPS[p.sim.melee_swing().unwrap_or(0).min(MELEE_CLIPS.len() - 1)],
        MoveState::Smash => "smash_intro",
        MoveState::SmashLand => "smash",
        MoveState::WallStick if rig.wall_looping => wall_slide,
        MoveState::WallStick => wall_stick,
        MoveState::WallJump => wall_jump,
        MoveState::LedgeHang => match p.sim.ledge_move() {
            Some(LedgeMove::Grab) => "ledge_grab",
            Some(LedgeMove::FromFall) => rig.ledge_catch,
            Some(LedgeMove::UpTo) => "ledge_up",
            Some(LedgeMove::DownTo) => "ledge_down_to",
            None => "ledge_idle",
        },
        // The climb up a caught ledge (Ledge_Jump_Up_InAir), the let-go with a turn, and the
        // ledge jump, which the game animates with the double jump's flip (Jump_In_Air).
        MoveState::LedgeClimb => "ledge_jump",
        MoveState::LedgeDrop => "ledge_drop_turn",
        MoveState::LedgeJump => "double_jump",
        // The Swingshot: pulled in (near the ground), the first swing, then the loop for the way
        // he's swinging and how fast (the game's graph picks these; ours by his speed along his
        // facing); the jump off; the Hookshot's reel.
        MoveState::Swing => match p.sim.swing_phase() {
            Some(0) if p.contact.ground_distance.is_some_and(|d| d < 0.9 * settings.scale) => "swing_pull",
            Some(0) => "swing_intro",
            _ => {
                let yaw = p.sim.facing_yaw();
                let ahead = Vec2::new(v.x, v.z).dot(Vec2::new(yaw.sin(), yaw.cos()));
                match (ahead >= 0.0, Vec2::new(v.x, v.z).length() > SWING_FAST) {
                    (true, true) => "swing_forward_fast",
                    (true, false) => "swing_forward_slow",
                    (false, true) => "swing_backward_fast",
                    (false, false) => "swing_backward_slow",
                }
            }
        },
        MoveState::Hook => "hook_reel",
        MoveState::SwingJump => "swing_jump",
    };
    let want = match carried(want) {
        c if carrying && rig.clips.contains_key(c) => c,
        _ => want,
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
        let quick = |c: Option<&str>| c.is_some_and(|c| MELEE_CLIPS.contains(&c) || c.starts_with("smash") || c.contains("strafe"));
        let landing = |c: Option<&str>| c.is_some_and(|c| c.starts_with("land") || c.starts_with("wrench_land"));
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
    // The sounds the clip calls for, as its time passes them (looping clips wrap around). A
    // clip that just started sounds what it passed already. The jump clip's landing sound plays
    // at landing instead (simulate), not at the end of the clip in the air.
    if let Some(&node) = rig.clips.get(want)
        && let Some(now) = player.animation(node).map(|a| a.seek_time())
    {
        let from = match rig.sound_clock {
            Some((clip, t)) if clip == want => Some(t),
            _ => None,
        };
        rig.sound_clock = Some((want, now));
        if let Some(list) = rig.clip_sounds.get(want) {
            for (t, name) in list {
                let passed = match from {
                    Some(f) if now >= f => *t > f && *t <= now,
                    Some(f) => *t > f || *t <= now,
                    None => *t <= now,
                };
                if passed && name != "Play_FOL_ratchet_land" {
                    cues.write(crate::sound::SoundCue::play(name.clone()));
                }
            }
        }
    }

    // Match the walk, run and strafe cycles (and the moving landings) to the actual speed: at
    // the speed a clip was authored for, it plays at 1x.
    if (SPEED_MATCHED.contains(&want) || (want.contains("strafe") && !want.contains("jump") && !want.contains("land")))
        && let Some(&node) = rig.clips.get(want)
    {
        let tuning = p.sim.tuning();
        let authored = rig.authored_speed.get(want).copied().unwrap_or_else(|| {
            (if want.contains("strafe_walk") { tuning.aim_speeds.run_max } else { tuning.ground_speeds.run_max }) / settings.scale
        });
        if let Some(active) = player.animation_mut(node) {
            let range = if want.contains("land") { (0.7, 1.3) } else { (0.5, 1.5) };
            active.set_speed((speed / authored.max(0.1)).clamp(range.0, range.1));
        }
    }
    // The wall jump's in-air clip plays at a quarter speed, the slowest the game plays a jump
    // clip: it speeds it up only to finish as he lands, and between walls it never does, so the
    // 0.2 s clip lasts the flight (RiftApart.exe). Past its end it holds its last pose.
    if want == wall_jump
        && let Some(&node) = rig.clips.get(want)
        && let Some(active) = player.animation_mut(node)
    {
        active.set_speed(WALL_JUMP_CLIP_RATE);
    }
    rig.walking = if rig.walking { speed < WALK_TO_RUN } else { speed < RUN_TO_WALK };
    lean_into_turns(&mut rig, &mut player, p.sim.facing_yaw(), want, time.delta_secs());
    // The glide leans into turns: its lean poses scrubbed by the turn rate.
    if let Some((node, duration)) = rig.glide_lean {
        let dt = time.delta_secs();
        let gliding = state == MoveState::Glide;
        let target = if gliding { (rig.turn_rate / FULL_LEAN_TURN_RATE).clamp(-1.0, 1.0) } else { 0.0 };
        rig.glide_lean_value += (target - rig.glide_lean_value) * (1.0 - (-dt / 0.2).exp());
        rig.glide_lean_weight = if gliding { (rig.glide_lean_weight + dt / 0.25).min(1.0) } else { (rig.glide_lean_weight - dt / 0.25).max(0.0) };
        if rig.glide_lean_weight > 0.0 {
            let t = (duration / 2.0 * (1.0 - rig.glide_lean_value)).clamp(0.0, duration * 0.999);
            let anim = if player.animation(node).is_some() { player.animation_mut(node) } else { Some(player.start(node).repeat()) };
            if let Some(anim) = anim {
                anim.set_speed(0.0).seek_to(t).set_weight(rig.glide_lean_weight);
            }
        } else if player.animation(node).is_some() {
            player.stop(node);
        }
    }
    // Fidgets play once, then it's back to the idle (and a while until the next one).
    let idling = matches!(want, "idle" | "wrench_idle");
    rig.idle_time = if idling { rig.idle_time + time.delta_secs() } else { rig.idle_time };
    if FIDGETS.contains(&want)
        && let Some(&node) = rig.clips.get(want)
        && player.animation(node).is_none_or(|a| a.is_finished())
    {
        rig.idle_time = 0.0;
        rig.fidgets_played += 1;
    } else if !FIDGETS.contains(&want) && !idling {
        rig.idle_time = 0.0;
    }
}

/// Blends the run's lean-left and lean-right clips in by how fast he turns, kept in step with
/// the run (same length, same time), and fades them out when he stops running. `clip` is the
/// move clip playing.
fn lean_into_turns(rig: &mut Rig, player: &mut AnimationPlayer, yaw: f32, clip: &str, dt: f32) {
    if rig.lean_nodes.is_empty() {
        return;
    }
    let running = rig.lean_nodes.iter().any(|(run, _)| *run == clip);
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
    rig.turn_rate = rate;
    // Turning toward +yaw is to his left.
    let target = if running { (rate / FULL_LEAN_TURN_RATE).clamp(-1.0, 1.0) } else { 0.0 };
    rig.lean += (target - rig.lean) * (1.0 - (-dt / 0.15).exp());
    rig.lean_blend += ((running as u8 as f32) - rig.lean_blend) * (1.0 - (-dt / 0.08).exp());
    let weights = [rig.lean.max(0.0), (-rig.lean).max(0.0)];
    for (run_clip, nodes) in rig.lean_nodes.clone() {
        // Each run's leans follow it, also while it fades out.
        let run = rig.clips.get(run_clip).and_then(|&n| player.animation(n)).map(|a| (a.seek_time(), a.speed(), a.weight()));
        for (node, w) in nodes.into_iter().zip(weights) {
            // The transitions hold the run's own weight at 1, so a lean weighted w/(1−w) takes
            // its share w of the blend.
            let w = w * rig.lean_blend;
            let w = w.min(0.95) / (1.0 - w.min(0.95));
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
    }
}

/// The first time of `event` in a converted clip (the converter's ratchet.json).
pub fn clip_event(manifest: &std::path::Path, clip: &str, event: &str) -> Option<f32> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(manifest).ok()?).ok()?;
    v["animations"][clip]["events"][event].get(0)?.as_f64().map(|t| t as f32)
}

/// Each clip's sounds from the converter's manifests: Ratchet's clips, plus for the swings the
/// wrench's own clips (which sound the swing in the game); a sound both call for at nearly the
/// same time plays once. Footsteps are the clips' footstep events.
fn clip_sounds(manifest: &std::path::Path, wrench: &std::path::Path) -> HashMap<String, Vec<(f32, String)>> {
    let read = |path: &std::path::Path| {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).unwrap_or_default()
    };
    let (ours, theirs) = (read(manifest), read(wrench));
    let mut out: HashMap<String, Vec<(f32, String)>> = HashMap::new();
    for (doc, only_swings) in [(&ours, false), (&theirs, true)] {
        for (name, a) in doc["animations"].as_object().into_iter().flatten() {
            if only_swings && !MELEE_CLIPS.contains(&name.as_str()) {
                continue;
            }
            let list = out.entry(name.clone()).or_default();
            for s in a["sounds"].as_array().into_iter().flatten() {
                if let (Some(t), Some(event)) = (s[0].as_f64(), s[1].as_str()) {
                    let t = t as f32;
                    if !list.iter().any(|(u, e)| e == event && (u - t).abs() < 0.1) {
                        list.push((t, event.to_string()));
                    }
                }
            }
            // A step at the very end of a looping clip is the one at its start.
            let end = a["seconds"].as_f64().unwrap_or(f64::MAX) as f32;
            for t in a["events"]["Footstep"].as_array().into_iter().flatten().filter_map(|t| t.as_f64()) {
                let t = t as f32;
                let near = |u: f32| (u - t).abs() < 0.1 || (t > end - 0.05 && u < 0.05);
                if !list.iter().any(|(u, e)| e == "footstep" && near(*u)) {
                    list.push((t, "footstep".to_string()));
                }
            }
        }
    }
    out.retain(|_, l| !l.is_empty());
    out
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
        MoveState::Dash
        | MoveState::AirDash
        | MoveState::Melee
        | MoveState::Smash
        | MoveState::SmashLand
        | MoveState::WallStick
        | MoveState::WallJump
        | MoveState::LedgeHang
        | MoveState::LedgeClimb
        | MoveState::LedgeDrop
        | MoveState::LedgeJump
        | MoveState::Swing
        | MoveState::Hook
        | MoveState::SwingJump => Pose {
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
