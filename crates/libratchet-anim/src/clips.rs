//! The clip names and the rules on them, as the sandbox's `ratchet_model.rs` has them: the move
//! clips, the layers, which clips loop, which play at the rate that keeps the feet planted, and
//! the crossfade times. Names are the ones in the converter's `ratchet.glb` (additive clips end
//! in `_additive`).

use libratchet::MoveState;

/// Clip names baked into ratchet.glb by the converter (one per move).
pub(crate) const CLIP_STATES: &[&str] = &[
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
    "swing_forward_slow", "swing_idle", "swing_backward_fast", "swing_backward_slow", "swing_jump", "hook_reel",
];
/// Dashes by direction relative to where he faces (clockwise from ahead), on the ground and in
/// the air: the game's Dash and Air_Dash drivers have one clip per direction.
pub(crate) const DASH_CLIPS: [&str; 8] = ["dash", "dash_ne", "dash_e", "dash_se", "dash_s", "dash_sw", "dash_w", "dash_nw"];
pub(crate) const AIR_DASH_CLIPS: [&str; 8] = ["air_dash", "air_dash_ne", "air_dash_e", "air_dash_se", "air_dash_s", "air_dash_sw", "air_dash_w", "air_dash_nw"];
/// The runs' lean-into-a-turn versions (left, right), blended in step with them by how fast he
/// turns: empty-handed and with the wrench.
pub(crate) const LEAN_CLIPS: [(&str, [&str; 2]); 2] =
    [("run", ["run_lean_left", "run_lean_right"]), ("wrench_run", ["wrench_run_lean_left", "wrench_run_lean_right"])];
/// The glove's hand pose (fingers, wrist and grip), over whatever he does while the glove is on.
/// It's a pose of those joints only, weighted far over the move clips under it (ours).
pub(crate) const GLOVE_HAND: &str = "glove_hand";
pub(crate) const GLOVE_HAND_WEIGHT: f32 = 20.0;
/// Drawing a weapon (additive, the game's Weapon_Draw driver): for the one-handed gun, the
/// wrench and the glove, each whole-body (standing) and upper-body (moving).
pub(crate) const DRAW_LAYERS: [[&str; 2]; 3] = [
    ["draw_pistol_additive", "draw_pistol_arm_additive"],
    ["draw_wrench_additive", "draw_wrench_arm_additive"],
    ["draw_glove_additive", "draw_glove_arm_additive"],
];
/// The draw fades in and out over these (the driver's blend times).
pub(crate) const DRAW_FADE_IN: f32 = 0.15;
pub(crate) const DRAW_FADE_OUT: f32 = 0.25;
/// Jumping while strafing (aiming or firing): by the jump's direction relative to where he faces
/// (n ahead, e his right, s back, w his left; the sideways and backward ones flip), then
/// standing still, for the one-handed grip and the wrench/glove grip. Double jumps have no
/// standing version (ahead), and landings their own set.
pub(crate) const STRAFE_JUMPS: [[&str; 5]; 2] = [
    ["strafe_jump_n", "strafe_jump_e", "strafe_jump_s", "strafe_jump_w", "strafe_jump_idle"],
    ["wrench_strafe_jump_n", "wrench_strafe_jump_e", "wrench_strafe_jump_s", "wrench_strafe_jump_w", "strafe_jump_idle"],
];
pub(crate) const STRAFE_DOUBLE_JUMPS: [[&str; 5]; 2] = [
    ["strafe_double_jump_n", "strafe_double_jump_e", "strafe_double_jump_s", "strafe_double_jump_w", "strafe_double_jump_n"],
    ["wrench_strafe_double_jump_n", "wrench_strafe_double_jump_e", "wrench_strafe_double_jump_s", "wrench_strafe_double_jump_w", "wrench_strafe_double_jump_n"],
];
pub(crate) const STRAFE_LANDINGS: [[&str; 5]; 2] = [
    ["strafe_land_n", "strafe_land_e", "strafe_land_s", "strafe_land_w", "strafe_land_idle"],
    ["wrench_strafe_land_n", "wrench_strafe_land_e", "wrench_strafe_land_s", "wrench_strafe_land_w", "wrench_strafe_land_idle"],
];
/// The glide's lean (additive, 11 poses, the middle one upright), scrubbed by how fast he turns,
/// fully at FULL_LEAN_TURN_RATE.
pub(crate) const GLIDE_LEAN: &str = "glide_lean_additive";
/// Turn rate (rad/s) at which the run leans fully into the turn (ours).
pub(crate) const FULL_LEAN_TURN_RATE: f32 = 5.0;
/// Walk below this speed, run above that one, with some overlap (m/s at Ratchet's size; between
/// the game's walk and run speed bands; ours).
pub(crate) const RUN_TO_WALK: f32 = 2.4;
pub(crate) const WALK_TO_RUN: f32 = 3.0;
/// Idle fidgets, after standing still this long (ours).
pub(crate) const FIDGETS: &[&str] = &["idle_look", "idle_shoe"];
pub(crate) const FIDGET_AFTER: f32 = 7.0;
/// Catching the thrown wrench: a full-body clip of which only the upper body (the `spine_1`
/// branch) is used, weighted well over the move clip under it.
pub(crate) const CATCH_CLIP: &str = "throw_catch";
pub(crate) const CATCH_WEIGHT: f32 = 6.0;
/// The game's grenade throws (underhand, additive), layered over whatever Ratchet does: the
/// whole-body one standing still, the upper-body one on the move or in the air.
pub(crate) const GRENADE_LAYERS: [&str; 2] = ["grenade_throw_additive", "grenade_throw_arm_additive"];
/// Winding a grenade throw up while the fire button is held (the same two versions).
pub(crate) const WINDUP_LAYERS: [&str; 2] = ["grenade_windup_additive", "grenade_windup_arm_additive"];
/// The aim offsets (additive, 21 poses scrubbed by the aim angle): one-handed and wrench/glove
/// grip, standing and running, each up/down (v) and left/right (h).
pub(crate) const AIM_LAYERS: [&str; 8] = [
    "aim_pistol_v_additive", "aim_pistol_h_additive", "aim_pistol_run_v_additive", "aim_pistol_run_h_additive", "aim_wrench_v_additive",
    "aim_wrench_h_additive", "aim_wrench_run_v_additive", "aim_wrench_run_h_additive",
];
/// Aim angles at the first and last pose (degrees). The middle pose aims straight ahead. Measured
/// from the pistol's poses (its arm reaches +87°/-45° and -64°/+96°); the game's own
/// parameter ranges weren't decoded, and the wrench/glove poses are assumed to match.
pub(crate) const AIM_UP: f32 = 90.0;
pub(crate) const AIM_DOWN: f32 = 45.0;
pub(crate) const AIM_SIDE: f32 = 90.0;
/// Strafing while aiming, by direction relative to where Ratchet faces (clockwise from ahead).
pub(crate) const STRAFE_CLIPS: &[&str] = &["strafe_n", "strafe_ne", "strafe_e", "strafe_se", "strafe_s", "strafe_sw", "strafe_w", "strafe_nw"];
/// The walking versions (aim mode's pace).
pub(crate) const STRAFE_WALK_CLIPS: &[&str] =
    &["strafe_walk_n", "strafe_walk_ne", "strafe_walk_e", "strafe_walk_se", "strafe_walk_s", "strafe_walk_sw", "strafe_walk_w", "strafe_walk_nw"];
pub(crate) const WRENCH_STRAFE_WALK_CLIPS: &[&str] = &[
    "wrench_strafe_walk_n", "wrench_strafe_walk_ne", "wrench_strafe_walk_e", "wrench_strafe_walk_se", "wrench_strafe_walk_s", "wrench_strafe_walk_sw",
    "wrench_strafe_walk_w", "wrench_strafe_walk_nw",
];
/// The same with the wrench or the glove (grenades) in hand.
pub(crate) const WRENCH_STRAFE_CLIPS: &[&str] = &[
    "wrench_strafe_n", "wrench_strafe_ne", "wrench_strafe_e", "wrench_strafe_se", "wrench_strafe_s", "wrench_strafe_sw", "wrench_strafe_w",
    "wrench_strafe_nw",
];
/// The one-handed fire recoil: an additive layer, restarted with every shot (when the model has
/// one; the converter leaves it out for now).
pub(crate) const FIRE_LAYER: &str = "fire_pistol_additive";
/// The wrench throw is an additive clip in the game (offsets on top of whatever Ratchet is
/// doing), so it plays as a layer over the move clips.
pub(crate) const THROW_LAYER: &str = "throw_additive";
/// Fade in/out of the throw layer, seconds.
pub(crate) const LAYER_FADE_IN: f32 = 0.08;
pub(crate) const LAYER_FADE_OUT: f32 = 0.25;
pub(crate) const MELEE_CLIPS: &[&str] = &["melee_1", "melee_2", "melee_3"];
/// The Swingshot's loop (the game's Swingshot_Loop): one blend of these clips by the swing's
/// Lean (0 swinging ahead, 0.5 stalled at an end, 1 swinging back), each at its Lean, faded in
/// and out as one move clip under the name SWING_LOOP (see `Animator::blend_swing_loop`).
/// (Assumption: the game's blend node layout isn't decoded; its data has Lean breakpoints at 0,
/// 0.333, 0.5, 0.666 and 1, read here as a line from swinging ahead fast through the stall to
/// swinging back fast.)
pub(crate) const SWING_LOOP: &str = "swing_loop";
pub(crate) const SWING_LOOP_CLIPS: [(f32, &str); 5] =
    [(0.0, "swing_forward_fast"), (0.333, "swing_forward_slow"), (0.5, "swing_idle"), (0.666, "swing_backward_slow"), (1.0, "swing_backward_fast")];
/// The wall jump's in-air clip plays at this rate (see `Animator::update`).
pub(crate) const WALL_JUMP_CLIP_RATE: f32 = 0.25;
pub(crate) const LOOPING: &[&str] = &[
    "idle", "run", "fall", "glide", "aim_idle", "strafe_n", "strafe_ne", "strafe_e", "strafe_se", "strafe_s", "strafe_sw", "strafe_w", "strafe_nw",
    "pistol_idle", "pistol_run", "walk", "glove_idle", "wrench_aim_idle", "wrench_strafe_n", "wrench_strafe_ne", "wrench_strafe_e",
    "wrench_strafe_se", "wrench_strafe_s", "wrench_strafe_sw", "wrench_strafe_w", "wrench_strafe_nw", "strafe_walk_n", "strafe_walk_ne", "strafe_walk_e",
    "strafe_walk_se", "strafe_walk_s", "strafe_walk_sw", "strafe_walk_w", "strafe_walk_nw", "wrench_strafe_walk_n", "wrench_strafe_walk_ne",
    "wrench_strafe_walk_e", "wrench_strafe_walk_se", "wrench_strafe_walk_s", "wrench_strafe_walk_sw", "wrench_strafe_walk_w", "wrench_strafe_walk_nw",
    "wrench_idle", "wrench_walk", "wrench_run", "wrench_glide", "glove_glide", "glide_pistol", "wall_slide_left", "wall_slide_right",
    "ledge_idle", SWING_LOOP, "hook_reel",
];
/// Played at the rate that keeps the feet planted.
pub(crate) const SPEED_MATCHED: &[&str] = &["run", "walk", "pistol_run", "land_walk", "land_run", "wrench_run", "wrench_walk", "wrench_land_walk", "wrench_land_run"];

/// With the glove on, or the melee grip during swings and throws (the game's thrown and melee
/// grips), the moves that have wrench-carrying versions use them, as the game's animset picks.
pub(crate) fn carried(clip: &'static str) -> &'static str {
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

/// On a wall, a ledge or the Swingshot, or jumping off one: the game holsters the weapon (the
/// sandbox's `weapons::holstered`).
pub(crate) fn holstered(state: MoveState) -> bool {
    matches!(
        state,
        MoveState::WallStick
            | MoveState::WallJump
            | MoveState::LedgeHang
            | MoveState::LedgeClimb
            | MoveState::LedgeDrop
            | MoveState::LedgeJump
            | MoveState::Swing
            | MoveState::Hook
            | MoveState::SwingJump
    )
}

/// The crossfade (ms) from the clip playing to the one picked: the Swingshot's own, then quick
/// cuts for wrench moves, the grenade throw and strafe changes, landings, the slow fidgets, and
/// the rest.
pub(crate) fn fade_ms(from: Option<&str>, to: &str) -> u64 {
    // Wrench moves, the grenade throw and strafe changes cut in quickly.
    let quick = |c: Option<&str>| c.is_some_and(|c| MELEE_CLIPS.contains(&c) || c.starts_with("smash") || c.contains("strafe"));
    let landing = |c: Option<&str>| c.is_some_and(|c| c.starts_with("land") || c.starts_with("wrench_land"));
    if let Some(ms) = swing_blend_ms(from, to) {
        ms
    } else if quick(Some(to)) || quick(from) {
        60
    } else if landing(Some(to)) || landing(from) {
        90
    } else if FIDGETS.contains(&to) || from.is_some_and(|c| FIDGETS.contains(&c)) {
        400
    } else {
        160
    }
}

/// The Swingshot's blend times (ms) from one clip to the next, from the game's transition
/// tables (RiftApart.exe): onto a swing (the pull in or the intro, from the jump or fall pose)
/// and into the Hookshot's reel 0.25 s; the intro into the loop 0.35 s; off with a jump, a cut
/// from the swing's first clip and 0.1 s from the loop; letting go into the fall, 0.25 s from
/// the first clip and 0.5 s from the loop; the swing jump into the fall at its top, 0.4 s.
pub(crate) fn swing_blend_ms(from: Option<&str>, to: &str) -> Option<u64> {
    let first = from.is_some_and(|c| matches!(c, "swing_pull" | "swing_intro"));
    let looping = from == Some(SWING_LOOP);
    Some(match to {
        // Ours: hooking on mid-flip, the game's 0.25 s blend from the swing jump turns his body
        // over (its poses are half a turn apart); a short one keeps that to a frame or two.
        "swing_pull" | "swing_intro" if from == Some("swing_jump") => 100,
        "swing_pull" | "swing_intro" | "hook_reel" => 250,
        SWING_LOOP => 350,
        "swing_jump" if first => 0,
        "swing_jump" if looping => 100,
        "fall" if first => 250,
        "fall" if looping => 500,
        "fall" if from == Some("swing_jump") => 400,
        _ => return None,
    })
}

/// Each swing loop clip's share at this Lean (see SWING_LOOP_CLIPS): the two around it, by how
/// close it is to each.
pub(crate) fn swing_loop_weights(lean: f32) -> [f32; 5] {
    let lean = lean.clamp(0.0, 1.0);
    let mut weights = [0.0; 5];
    let i = SWING_LOOP_CLIPS.windows(2).position(|w| lean <= w[1].0).unwrap_or(SWING_LOOP_CLIPS.len() - 2);
    let (a, b) = (SWING_LOOP_CLIPS[i].0, SWING_LOOP_CLIPS[i + 1].0);
    let t = ((lean - a) / (b - a)).clamp(0.0, 1.0);
    weights[i] = 1.0 - t;
    weights[i + 1] = t;
    weights
}

/// Every clip name the animator looks for, each once: the move clips, then the layers.
pub fn clip_names() -> Vec<&'static str> {
    let layers = LEAN_CLIPS.iter().flat_map(|(_, names)| names.iter().copied());
    let layers = layers.chain(DRAW_LAYERS.iter().flatten().copied()).chain([GLIDE_LEAN, GLOVE_HAND, CATCH_CLIP]);
    let layers = layers.chain(GRENADE_LAYERS).chain(WINDUP_LAYERS).chain(AIM_LAYERS).chain([THROW_LAYER, FIRE_LAYER]);
    CLIP_STATES.iter().copied().chain(layers).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_swing_loop_blends_the_two_clips_around_its_lean() {
        // On a breakpoint, only that clip; between two, both by how close.
        for (i, (lean, _)) in SWING_LOOP_CLIPS.into_iter().enumerate() {
            let w = swing_loop_weights(lean);
            assert!((w[i] - 1.0).abs() < 1e-5 && (w.iter().sum::<f32>() - 1.0).abs() < 1e-5, "at {lean}: {w:?}");
        }
        let w = swing_loop_weights(0.25);
        assert!((w[0] - 0.25).abs() < 1e-2 && (w[1] - 0.75).abs() < 1e-2 && w[2..].iter().all(|w| *w == 0.0), "{w:?}");
        // Out of range, the ends.
        assert_eq!(swing_loop_weights(-1.0)[0], 1.0);
        assert_eq!(swing_loop_weights(2.0)[4], 1.0);
    }

    #[test]
    fn the_swing_jump_cuts_from_the_intro_and_blends_from_the_loop() {
        assert_eq!(swing_blend_ms(Some("swing_intro"), "swing_jump"), Some(0));
        assert_eq!(swing_blend_ms(Some(SWING_LOOP), "swing_jump"), Some(100));
        assert_eq!(swing_blend_ms(Some("swing_intro"), SWING_LOOP), Some(350));
        assert_eq!(swing_blend_ms(Some("swing_jump"), "fall"), Some(400));
        assert_eq!(swing_blend_ms(Some("jump"), "swing_intro"), Some(250));
        // Other moves keep their own.
        assert_eq!(swing_blend_ms(Some("run"), "fall"), None);
    }

    #[test]
    fn fades_by_what_is_left_and_what_comes() {
        // Strafe changes and wrench moves first, then landings, fidgets, and the rest.
        assert_eq!(fade_ms(Some("strafe_n"), "strafe_ne"), 60);
        assert_eq!(fade_ms(Some("idle"), "melee_1"), 60);
        assert_eq!(fade_ms(Some("smash_intro"), "smash"), 60);
        assert_eq!(fade_ms(Some("fall"), "strafe_land_n"), 60);
        assert_eq!(fade_ms(Some("fall"), "land"), 90);
        assert_eq!(fade_ms(Some("wrench_land_run"), "wrench_run"), 90);
        assert_eq!(fade_ms(Some("idle"), "idle_look"), 400);
        assert_eq!(fade_ms(Some("idle_shoe"), "run"), 400);
        assert_eq!(fade_ms(Some("idle"), "run"), 160);
        assert_eq!(fade_ms(None, "idle"), 160);
        // The Swingshot's own come first.
        assert_eq!(fade_ms(Some("swing_intro"), "swing_jump"), 0);
    }

    #[test]
    fn every_name_is_listed_once() {
        let names = clip_names();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
        assert!(names.contains(&"glove_hand") && names.contains(&"throw_catch") && names.contains(&"aim_wrench_run_h_additive"));
        assert!(!names.contains(&SWING_LOOP), "the swing loop isn't a clip");
    }
}
