//! Scripted demo: plays a fixed run through the moves, takes screenshots, optionally saves
//! every other frame for a video, then quits. Lets the sandbox be checked with nobody at the
//! keyboard, and gives repeatable before/after comparisons.

use crate::level::spots;
use crate::{CameraRig, Player, ScriptedInput, Settings, Trail, respawn, simulate};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use libratchet::Input as MoveInput;
use std::path::PathBuf;

const TICK: f32 = 1.0 / 120.0;

#[derive(Clone)]
enum Act {
    /// Hold this input for `secs`. Direction is world-space (x, z).
    Hold { secs: f32, dir: Vec2, jump: bool, dash: bool, melee: bool },
    /// Throw the wrench (straight ahead).
    Throw,
    /// Pick a weapon.
    Slot(crate::weapons::Slot),
    /// Gun controls from here on: trigger held, aiming.
    Gun { trigger: bool, aim: bool },
    Teleport(Vec3),
    ClearTrail,
    Scale(f32),
    /// Follow-camera angles (also where guns aim, and back from `Watch`).
    Follow { yaw: f32, pitch: f32, distance: f32 },
    /// Close-up framing (the camera looks at his chest, not the game's pivot above him).
    CloseUp(bool),
    /// Watch from a fixed camera from here on, without stopping.
    Watch { focus: Vec3, yaw: f32, pitch: f32, distance: f32 },
    /// Guns aim at this point instead of where the camera looks.
    AimAt(Option<Vec3>),
    /// Stand still, look at a fixed spot for `secs` and take a screenshot.
    Wide { name: &'static str, focus: Vec3, yaw: f32, pitch: f32, distance: f32, secs: f32 },
    /// Screenshot with the current (follow) camera, without stopping.
    Shot(&'static str),
}

#[derive(Resource)]
struct Demo {
    acts: Vec<Act>,
    index: usize,
    elapsed: f32,
    shot_taken: bool,
    out: PathBuf,
    record: Option<PathBuf>,
    frame: u32,
    finished_for: f32,
    /// A screenshot to take on the next frame the recorder isn't saving (two screenshots in one
    /// frame make Bevy drop one).
    pending_shot: Option<PathBuf>,
}

/// `part`: run only one section ("wrench"), after a short warm-up.
pub fn add(app: &mut App, out: PathBuf, record: Option<PathBuf>, part: Option<&str>) {
    let acts = match part {
        Some("wrench") => [vec![hold(1.5, Vec2::ZERO, false)], wrench_moves()].concat(),
        Some("weapons") => [vec![hold(1.5, Vec2::ZERO, false)], weapon_moves()].concat(),
        Some("moves") => [vec![hold(1.5, Vec2::ZERO, false)], locomotion()].concat(),
        Some("combat") => [vec![hold(1.5, Vec2::ZERO, false)], combat()].concat(),
        Some("face") => [vec![hold(1.5, Vec2::ZERO, false)], face()].concat(),
        _ => script(),
    };
    app.insert_resource(Demo { acts, index: 0, elapsed: 0.0, shot_taken: false, out, record, frame: 0, finished_for: 0.0, pending_shot: None })
        .add_systems(FixedUpdate, run_script.before(simulate))
        .add_systems(Update, record_frames);
}

fn hold(secs: f32, dir: Vec2, jump: bool) -> Act {
    Act::Hold { secs, dir, jump, dash: false, melee: false }
}

fn dash(dir: Vec2) -> Act {
    Act::Hold { secs: 2.0 * TICK, dir, jump: false, dash: true, melee: false }
}

/// A melee press (a swing on the ground, a Hyper-Strike in the air).
fn swing() -> Act {
    Act::Hold { secs: 2.0 * TICK, dir: Vec2::ZERO, jump: false, dash: false, melee: true }
}

/// The OmniWrench in the arena: the 3-hit combo into the first dummies, a Hyper-Strike into the
/// ring further on, and a throw at the far one. Ratchet faces +X.
fn wrench_moves() -> Vec<Act> {
    let arena = spots::ARENA;
    let x = Vec2::X;
    vec![
        Act::ClearTrail,
        // Framed on him rather than the game's pivot, to see the swings.
        Act::CloseUp(true),
        Act::Teleport(arena + Vec3::new(-1.0, 0.0, 0.0)),
        Act::Follow { yaw: 0.75, pitch: 0.2, distance: 5.0 },
        hold(0.12, x, false),
        hold(0.9, Vec2::ZERO, false),
        Act::Shot("13_wrench_ready"),
        hold(0.1, Vec2::ZERO, false),
        // Swing 1 hits at about 0.05 s; a press during each swing queues the next.
        swing(),
        hold(0.07, Vec2::ZERO, false),
        Act::Shot("14_melee_1"),
        swing(),
        hold(0.18, Vec2::ZERO, false),
        Act::Shot("15_melee_2"),
        swing(),
        hold(0.42, Vec2::ZERO, false),
        Act::Shot("16_melee_3"),
        hold(1.2, Vec2::ZERO, false),
        // Hyper-Strike: jump, melee at the top, slam down among the ring of dummies.
        Act::Teleport(arena + Vec3::new(8.0, 0.0, 0.0)),
        hold(0.12, x, false),
        hold(0.5, Vec2::ZERO, false),
        Act::Follow { yaw: 1.1, pitch: 0.18, distance: 6.0 },
        hold(0.35, Vec2::ZERO, true),
        hold(0.05, Vec2::ZERO, false),
        swing(),
        hold(0.14, Vec2::ZERO, false),
        Act::Shot("17_hyper_strike_hop"),
        hold(0.2, Vec2::ZERO, false),
        Act::Shot("18_hyper_strike_slam"),
        hold(1.0, Vec2::ZERO, false),
        // Throw at the far dummy, 10 m ahead.
        Act::Teleport(arena + Vec3::new(12.0, 0.0, 4.0)),
        hold(0.12, x, false),
        hold(0.6, Vec2::ZERO, false),
        Act::Follow { yaw: 0.45, pitch: 0.15, distance: 4.0 },
        hold(0.3, Vec2::ZERO, false),
        Act::Throw,
        hold(0.2, Vec2::ZERO, false),
        Act::Shot("19_wrench_throw"),
        hold(0.3, Vec2::ZERO, false),
        Act::Follow { yaw: -1.3, pitch: 0.25, distance: 9.0 },
        hold(0.05, Vec2::ZERO, false),
        Act::Shot("20_wrench_hit"),
        hold(2.0, Vec2::ZERO, false),
        // Throwing on the run: the throw is a layer over the run cycle.
        Act::Follow { yaw: 1.4, pitch: 0.15, distance: 4.5 },
        hold(0.7, Vec2::Y, false),
        Act::Throw,
        hold(0.22, Vec2::Y, false),
        Act::Shot("21_running_throw"),
        hold(1.8, Vec2::Y, false),
        hold(0.8, Vec2::ZERO, false),
        Act::CloseUp(false),
    ]
}

/// Ground dash, then jump + air dash, heading +X.
fn dashes() -> Vec<Act> {
    let x = Vec2::X;
    vec![
        hold(0.5, Vec2::ZERO, false),
        hold(0.5, x, false),
        dash(x),
        hold(1.0, x, false),
        hold(0.24, x, true),
        hold(0.04, x, false),
        dash(x),
        hold(1.2, x, false),
        hold(0.4, Vec2::ZERO, false),
    ]
}

/// Run, tap jump, full jump, double jump, double jump + glide, all heading +X.
fn jump_types() -> Vec<Act> {
    let x = Vec2::X;
    vec![
        hold(0.5, Vec2::ZERO, false),
        hold(0.6, x, false),
        // Tap jump.
        hold(2.0 * TICK, x, true),
        hold(0.9, x, false),
        // Full jump.
        hold(0.35, x, true),
        hold(0.8, x, false),
        // Full jump, then double jump at the top.
        hold(0.24, x, true),
        hold(0.04, x, false),
        hold(0.15, x, true),
        hold(1.0, x, false),
        // Double jump and keep holding: glide.
        hold(0.24, x, true),
        hold(0.04, x, false),
        hold(1.8, x, true),
        hold(0.6, x, false),
        hold(0.4, Vec2::ZERO, false),
    ]
}

/// Walking, running into curves (the run leans into turns), a running landing and an idle
/// fidget.
fn locomotion() -> Vec<Act> {
    let x = Vec2::X;
    // A run curving through `turn` radians over `secs`, starting along `from` (+ = to his left).
    let curve = |from: Vec2, turn: f32, secs: f32| -> Vec<Act> {
        let steps = (secs / 0.05).round() as usize;
        (1..=steps).map(|i| hold(0.05, Vec2::from_angle(-turn * i as f32 / steps as f32).rotate(from), false)).collect()
    };
    let mut acts = vec![
        Act::ClearTrail,
        Act::CloseUp(true),
        Act::Teleport(spots::DEMO_LANE),
        Act::Follow { yaw: 1.2, pitch: 0.12, distance: 3.6 },
        hold(0.6, Vec2::ZERO, false),
        // Half stick: walking.
        hold(2.2, x * 0.5, false),
        Act::Shot("30_walk"),
        hold(0.6, x * 0.5, false),
        Act::CloseUp(false),
        Act::Follow { yaw: 1.9, pitch: 0.2, distance: 5.0 },
        hold(1.0, x, false),
        Act::Shot("31_run"),
    ];
    // Curving left, then right: the run leans into each turn.
    acts.extend(curve(x, 1.6, 0.6));
    acts.push(Act::Shot("32_lean_left"));
    acts.extend(curve(Vec2::from_angle(-1.6).rotate(x), -1.6, 0.6));
    acts.push(Act::Shot("33_lean_right"));
    acts.extend([
        hold(0.5, x, false),
        // A running jump and landing.
        hold(0.2, x, true),
        hold(0.5, x, false),
        Act::Shot("34_land_run"),
        hold(0.6, x, false),
        // Standing still long enough for a fidget.
        Act::CloseUp(true),
        Act::Follow { yaw: 1.0, pitch: 0.15, distance: 3.4 },
        hold(8.5, Vec2::ZERO, false),
        Act::Shot("35_fidget"),
        hold(3.0, Vec2::ZERO, false),
        Act::CloseUp(false),
    ]);
    acts
}

/// Close follow-camera shots of the character in each move (front-right view while running +X).
fn close_ups() -> Vec<Act> {
    let x = Vec2::X;
    vec![
        Act::ClearTrail,
        Act::CloseUp(true),
        Act::Teleport(spots::DEMO_LANE),
        Act::Follow { yaw: 1.2, pitch: 0.12, distance: 3.4 },
        hold(1.2, Vec2::ZERO, false),
        Act::Shot("05_close_idle"),
        // From behind, to see Clank on his back.
        Act::Follow { yaw: 2.7, pitch: 0.25, distance: 3.0 },
        hold(0.4, Vec2::ZERO, false),
        Act::Shot("05b_close_back"),
        Act::Follow { yaw: 1.2, pitch: 0.12, distance: 3.4 },
        hold(0.7, x, false),
        Act::Shot("06_close_run"),
        hold(0.12, x, false),
        Act::Shot("07_close_run_b"),
        hold(0.15, x, true),
        Act::Shot("08_close_jump"),
        hold(0.25, x, true),
        hold(0.04, x, false),
        hold(0.12, x, true),
        Act::Shot("09_close_double_jump"),
        hold(0.55, x, true),
        Act::Shot("10_close_glide"),
        Act::Follow { yaw: -2.3, pitch: 0.05, distance: 4.6 },
        hold(0.25, x, true),
        Act::Shot("10b_close_glide_back"),
        Act::Follow { yaw: 1.2, pitch: 0.12, distance: 3.4 },
        hold(0.6, x, false),
        hold(0.5, x, false),
        dash(x),
        hold(0.1, x, false),
        Act::Shot("11_close_dash"),
        hold(0.8, x, false),
        // A long glide off the tower, camera behind: Clank's heli-pack.
        Act::ClearTrail,
        Act::Teleport(spots::TOWER_TOP + Vec3::new(-1.5, 0.0, 0.0)),
        Act::Follow { yaw: -1.9, pitch: 0.2, distance: 3.6 },
        hold(0.6, Vec2::ZERO, false),
        hold(0.45, x, false),
        hold(0.22, x, true),
        hold(0.04, x, false),
        hold(1.1, x, true),
        Act::Shot("12_glide_heli_a"),
        Act::Follow { yaw: -1.2, pitch: 0.35, distance: 3.2 },
        hold(0.5, x, true),
        Act::Shot("12_glide_heli_b"),
        hold(1.6, x, true),
        hold(0.6, x, false),
        Act::CloseUp(false),
    ]
}

fn script() -> Vec<Act> {
    let lane = spots::DEMO_LANE;
    let top = spots::TOWER_TOP;
    // Give the renderer a moment to warm up before the first screenshot.
    let mut acts = vec![
        hold(1.5, Vec2::ZERO, false),
        Act::Wide { name: "00_course", focus: Vec3::new(-15.0, 0.0, 35.0), yaw: 0.5, pitch: 0.5, distance: 110.0, secs: 0.6 },
    ];

    acts.extend([Act::Teleport(lane), Act::ClearTrail, Act::Follow { yaw: 0.0, pitch: 0.2, distance: 9.0 }]);
    acts.extend(jump_types());
    acts.push(Act::Wide { name: "01_jump_types_ratchet", focus: lane + Vec3::new(19.0, 1.8, 0.0), yaw: 0.0, pitch: 0.06, distance: 30.0, secs: 1.5 });

    acts.extend([
        Act::ClearTrail,
        Act::Teleport(top + Vec3::new(-1.5, 0.0, 0.0)),
        Act::Follow { yaw: 0.0, pitch: 0.25, distance: 12.0 },
        hold(0.5, Vec2::ZERO, false),
        hold(0.45, Vec2::X, false),
        hold(0.22, Vec2::X, true),
        hold(0.04, Vec2::X, false),
        hold(4.5, Vec2::X, true),
        hold(0.8, Vec2::X, false),
        Act::Wide { name: "02_tower_glide", focus: top + Vec3::new(12.0, -5.0, 0.0), yaw: 0.0, pitch: 0.08, distance: 34.0, secs: 1.5 },
    ]);

    acts.extend([Act::ClearTrail, Act::Scale(crate::DRAGONBORN_SCALE), Act::Teleport(lane), Act::Follow { yaw: 0.0, pitch: 0.2, distance: 12.0 }]);
    acts.extend(jump_types());
    acts.push(Act::Wide { name: "03_jump_types_dragonborn", focus: lane + Vec3::new(31.0, 2.8, 0.0), yaw: 0.0, pitch: 0.06, distance: 48.0, secs: 1.5 });
    acts.push(Act::Scale(1.0));

    acts.extend([Act::ClearTrail, Act::Teleport(lane), Act::Follow { yaw: 0.0, pitch: 0.2, distance: 9.0 }]);
    acts.extend(dashes());
    acts.push(Act::Wide { name: "04_dashes", focus: lane + Vec3::new(14.0, 1.5, 0.0), yaw: 0.0, pitch: 0.06, distance: 24.0, secs: 1.5 });
    acts.extend(close_ups());
    acts.extend(wrench_moves());
    acts.extend(weapon_moves());
    acts
}

/// The Burst Pistol and the Shatterbomb on the dummies in the arena (Ratchet facing +X, the
/// camera behind him, aiming where it looks).
/// The playtest fixes: the combo at the game's timing (mashing, then a press just after a
/// swing), throwing the wrench while aiming (mid-jump too) and a Shatterbomb thrown on the walk.
/// Ratchet faces +X.
fn combat() -> Vec<Act> {
    use crate::weapons::Slot;
    let arena = spots::ARENA;
    let x = Vec2::X;
    // Aiming follows the camera: from behind (-X), he faces +X.
    let behind = -std::f32::consts::FRAC_PI_2;
    let start = arena + Vec3::new(-1.5, 0.0, 0.0);
    let mut acts = vec![
        Act::ClearTrail,
        Act::CloseUp(true),
        Act::Slot(Slot::Wrench),
        Act::Teleport(start),
        Act::Follow { yaw: 0.9, pitch: 0.2, distance: 5.5 },
        hold(0.12, x, false),
        hold(0.8, Vec2::ZERO, false),
    ];
    // Mashing, a press every 0.1 s: each swing hands over at its first window (0.2 s in), the
    // third plays out and the combo rests, then it starts over.
    for i in 0..30 {
        acts.push(swing());
        acts.push(hold(0.1 - 2.0 * TICK, Vec2::ZERO, false));
        match i {
            2 => acts.push(Act::Shot("40_mash_swing_2")),
            6 => acts.push(Act::Shot("41_mash_swing_3")),
            14 => acts.push(Act::Shot("42_combo_rest")),
            19 => acts.push(Act::Shot("43_combo_again")),
            _ => {}
        }
    }
    acts.extend([
        hold(1.4, Vec2::ZERO, false),
        // One swing, then a press 0.2 s after it ended: still the second swing.
        Act::Teleport(start),
        hold(0.12, x, false),
        hold(1.0, Vec2::ZERO, false),
        swing(),
        hold(0.75, Vec2::ZERO, false),
        swing(),
        hold(0.15, Vec2::ZERO, false),
        Act::Shot("45_late_press_swing_2"),
        hold(1.2, Vec2::ZERO, false),
        Act::CloseUp(false),
        // Aim, jump, and swing at the top: the wrench flies along the aim.
        Act::Teleport(arena + Vec3::new(12.0, 0.0, 4.0)),
        hold(0.12, x, false),
        Act::Follow { yaw: behind, pitch: 0.12, distance: 4.5 },
        Act::Gun { trigger: false, aim: true },
        hold(0.6, Vec2::ZERO, false),
        Act::Shot("46_aiming_with_the_wrench"),
        hold(0.3, Vec2::ZERO, true),
        Act::Throw,
        hold(0.15, Vec2::ZERO, false),
        Act::Shot("47_air_throw"),
        hold(1.8, Vec2::ZERO, false),
        Act::Gun { trigger: false, aim: false },
        // The pistol fired from the hip while backing away (S): he strafes facing the aim while
        // it fires, turns around to run once it stops (0.1 s after the last shot), and back.
        Act::Slot(Slot::BurstPistol),
        Act::Teleport(arena + Vec3::new(-3.5, 0.0, 0.0)),
        hold(0.12, x, false),
        Act::Follow { yaw: behind + 0.5, pitch: 0.15, distance: 5.0 },
        hold(0.4, Vec2::ZERO, false),
        Act::Gun { trigger: true, aim: false },
        hold(0.8, -x, false),
        Act::Shot("49_hip_fire_backing_away"),
        Act::Gun { trigger: false, aim: false },
        hold(0.2, -x, false),
        Act::Shot("49b_hip_fire_turning"),
        hold(0.7, -x, false),
        Act::Gun { trigger: true, aim: false },
        hold(0.8, -x, false),
        Act::Gun { trigger: false, aim: false },
        hold(0.6, Vec2::ZERO, false),
        // A Shatterbomb thrown while walking past the ring, filmed from his right.
        Act::Slot(Slot::Shatterbomb),
        Act::Teleport(arena + Vec3::new(1.0, 0.0, 3.5)),
        hold(0.12, x, false),
        Act::Follow { yaw: behind, pitch: 0.12, distance: 4.5 },
        Act::AimAt(Some(arena + Vec3::new(8.0, 0.3, 1.6))),
        Act::Watch { focus: arena + Vec3::new(3.5, 0.6, 3.5), yaw: 0.0, pitch: 0.12, distance: 7.0 },
        hold(0.4, Vec2::ZERO, false),
        hold(0.3, x * 0.45, false),
        // Held, it winds up and shows the arc; released, it goes.
        Act::Gun { trigger: true, aim: false },
        hold(0.6, x * 0.45, false),
        Act::Shot("48_shatterbomb_windup"),
        hold(0.1, x * 0.45, false),
        Act::Gun { trigger: false, aim: false },
        hold(0.15, x * 0.45, false),
        Act::Shot("48b_walking_shatterbomb"),
        hold(1.2, x * 0.45, false),
        hold(1.0, Vec2::ZERO, false),
        Act::AimAt(None),
        // Aim offsets: the pistol aimed high, then low; his arm and upper body follow.
        Act::Slot(Slot::BurstPistol),
        Act::Teleport(arena + Vec3::new(-3.0, 0.0, 6.0)),
        hold(0.12, x, false),
        Act::Follow { yaw: behind, pitch: 0.1, distance: 4.0 },
        Act::Watch { focus: arena + Vec3::new(-3.0, 0.8, 6.0), yaw: 0.3, pitch: 0.05, distance: 3.2 },
        Act::AimAt(Some(arena + Vec3::new(4.0, 6.0, 6.0))),
        Act::Gun { trigger: false, aim: true },
        hold(0.8, Vec2::ZERO, false),
        Act::Shot("4a_aim_high"),
        hold(0.1, Vec2::ZERO, false),
        Act::AimAt(Some(arena + Vec3::new(0.0, -2.0, 6.0))),
        hold(0.6, Vec2::ZERO, false),
        Act::Shot("4b_aim_low"),
        hold(0.1, Vec2::ZERO, false),
        Act::AimAt(Some(arena + Vec3::new(4.0, 0.9, 6.0))),
        hold(0.6, Vec2::ZERO, false),
        Act::Shot("4c_aim_level"),
        hold(0.1, Vec2::ZERO, false),
        Act::Gun { trigger: false, aim: false },
        Act::AimAt(None),
        hold(0.4, Vec2::ZERO, false),
        Act::Slot(Slot::Wrench),
        Act::Follow { yaw: 0.9, pitch: 0.2, distance: 9.0 },
    ]);
    acts
}

/// His face up close: standing, then through a jump and a double jump (facing the camera).
fn face() -> Vec<Act> {
    let front = Act::Follow { yaw: 0.0, pitch: -0.05, distance: 1.1 };
    vec![
        Act::CloseUp(true),
        Act::Teleport(spots::DEMO_LANE),
        hold(0.12, Vec2::Y, false),
        front,
        hold(1.5, Vec2::ZERO, false),
        Act::Shot("50_face_idle"),
        hold(0.1, Vec2::ZERO, true),
        hold(0.12, Vec2::ZERO, false),
        Act::Shot("51_face_jump"),
        hold(0.1, Vec2::ZERO, false),
        Act::Shot("52_face_jump_top"),
        hold(0.1, Vec2::ZERO, true),
        hold(0.15, Vec2::ZERO, false),
        Act::Shot("53_face_double_jump"),
        hold(1.5, Vec2::ZERO, false),
        Act::CloseUp(false),
    ]
}

fn weapon_moves() -> Vec<Act> {
    use crate::weapons::Slot;
    // South of the dummy ring, facing it (+Z), so nothing stands between him and the camera.
    let spot = spots::ARENA + Vec3::new(8.55, 0.0, -7.5);
    let z = Vec2::Y;
    let behind = std::f32::consts::PI;
    // A camera on his right (gun) side, and dummies to aim at from there.
    let right_side = -std::f32::consts::FRAC_PI_2;
    let dummy = spots::ARENA + Vec3::new(8.0, 1.1, -1.6);
    let far_dummy = spots::ARENA + Vec3::new(8.0, 0.3, 1.6);
    vec![
        Act::ClearTrail,
        Act::Teleport(spot),
        hold(0.12, z, false),
        hold(0.4, Vec2::ZERO, false),
        Act::Slot(Slot::BurstPistol),
        Act::Follow { yaw: behind, pitch: 0.06, distance: 4.5 },
        Act::Gun { trigger: false, aim: true },
        hold(0.6, Vec2::ZERO, false),
        Act::Shot("21_burst_pistol_aim"),
        Act::Gun { trigger: true, aim: true },
        hold(1.0, Vec2::ZERO, false),
        Act::Shot("22_burst_pistol_fire"),
        // Strafing to his right (-X when facing +Z) while firing.
        hold(0.6, Vec2::new(-1.0, 0.0), false),
        Act::Shot("23_strafe_fire"),
        hold(0.4, Vec2::new(-1.0, 0.0), false),
        Act::Gun { trigger: false, aim: false },
        hold(0.5, Vec2::ZERO, false),
        // From his right side, firing from the hip at the nearest dummy: bolts crossing the
        // screen, shells flying out of the gun.
        Act::Teleport(spot),
        hold(0.12, z, false),
        Act::Follow { yaw: behind, pitch: 0.06, distance: 4.5 },
        Act::AimAt(Some(dummy)),
        Act::Watch { focus: spot.lerp(dummy, 0.5) + Vec3::Y * 0.2, yaw: right_side, pitch: 0.1, distance: 5.0 },
        hold(0.3, Vec2::ZERO, false),
        Act::Gun { trigger: true, aim: false },
        hold(1.0, Vec2::ZERO, false),
        Act::Shot("24_pistol_side"),
        Act::Gun { trigger: false, aim: false },
        hold(1.2, Vec2::ZERO, false),
        Act::Shot("24b_shells"),
        // The same view for a Shatterbomb into the ring.
        Act::Slot(Slot::Shatterbomb),
        Act::AimAt(Some(far_dummy)),
        Act::Watch { focus: spot.lerp(far_dummy, 0.55) + Vec3::Y * 0.6, yaw: right_side, pitch: 0.15, distance: 7.5 },
        hold(0.4, Vec2::ZERO, false),
        Act::Gun { trigger: true, aim: true },
        hold(0.1, Vec2::ZERO, false),
        Act::Gun { trigger: false, aim: true },
        hold(0.75, Vec2::ZERO, false),
        Act::Shot("24c_shatterbomb_side"),
        hold(1.8, Vec2::ZERO, false),
        Act::Gun { trigger: false, aim: false },
        Act::AimAt(None),
        // Shatterbomb into the ring, from behind.
        Act::Slot(Slot::Shatterbomb),
        Act::Teleport(spot),
        hold(0.12, z, false),
        hold(0.4, Vec2::ZERO, false),
        Act::Follow { yaw: behind, pitch: 0.12, distance: 4.5 },
        Act::Gun { trigger: false, aim: true },
        hold(0.6, Vec2::ZERO, false),
        Act::Gun { trigger: true, aim: true },
        hold(0.1, Vec2::ZERO, false),
        Act::Gun { trigger: false, aim: true },
        hold(0.3, Vec2::ZERO, false),
        Act::Shot("25_shatterbomb_throw"),
        hold(0.45, Vec2::ZERO, false),
        Act::Shot("26_shatterbomb_blast"),
        hold(1.5, Vec2::ZERO, false),
        Act::Gun { trigger: false, aim: false },
        Act::Slot(Slot::Wrench),
        hold(0.5, Vec2::ZERO, false),
    ]
}

#[allow(clippy::too_many_arguments)]
fn run_script(
    time: Res<Time>,
    mut demo: ResMut<Demo>,
    mut scripted: ResMut<ScriptedInput>,
    mut throw: ResMut<crate::wrench::ScriptedThrow>,
    mut gun: ResMut<crate::weapons::ScriptedArsenal>,
    mut arsenal: Option<ResMut<crate::weapons::Arsenal>>,
    mut rig: ResMut<CameraRig>,
    mut settings: ResMut<Settings>,
    mut players: Query<&mut Player>,
    mut trail: ResMut<Trail>,
    mut exit: MessageWriter<AppExit>,
) {
    let dt = time.delta_secs();
    let Ok(mut p) = players.single_mut() else { return };
    loop {
        let Some(act) = demo.acts.get(demo.index).cloned() else {
            scripted.0 = Some(MoveInput::default());
            demo.finished_for += dt;
            // Give pending screenshots time to reach the disk.
            if demo.finished_for > 0.6 {
                exit.write(AppExit::Success);
            }
            return;
        };
        match act {
            Act::Teleport(at) => respawn(&mut p, at),
            Act::ClearTrail => *trail = Trail::default(),
            Act::Scale(s) => {
                settings.scale = s;
                let t = settings.base.scaled(s);
                p.sim.set_tuning(t);
            }
            Act::Follow { yaw, pitch, distance } => {
                rig.yaw = yaw;
                rig.pitch = pitch;
                rig.distance = distance;
                rig.fixed = None;
                rig.focus = p.feet;
            }
            Act::Throw => throw.0 = true,
            Act::Slot(slot) => {
                if let Some(a) = arsenal.as_mut() {
                    a.slot = slot;
                }
            }
            Act::Gun { trigger, aim } => *gun = crate::weapons::ScriptedArsenal { active: true, trigger, aim, target: gun.target },
            Act::AimAt(at) => gun.target = at,
            Act::CloseUp(on) => rig.close_up = on,
            Act::Watch { focus, yaw, pitch, distance } => rig.fixed = Some((focus, yaw, pitch, distance)),
            Act::Hold { secs, dir, jump, dash, melee } => {
                scripted.0 = Some(MoveInput { move_dir: dir, jump, dash, melee, aim: None, aim_mode: false });
                demo.elapsed += dt;
                if demo.elapsed >= secs - 1e-5 {
                    next(&mut demo);
                }
                return;
            }
            Act::Shot(name) => demo.pending_shot = Some(demo.out.join(format!("{name}.png"))),
            Act::Wide { name, focus, yaw, pitch, distance, secs } => {
                scripted.0 = Some(MoveInput::default());
                rig.fixed = Some((focus, yaw, pitch, distance));
                demo.elapsed += dt;
                if !demo.shot_taken && demo.elapsed >= 0.3 {
                    demo.pending_shot = Some(demo.out.join(format!("{name}.png")));
                    demo.shot_taken = true;
                }
                if demo.elapsed >= secs {
                    rig.fixed = None;
                    next(&mut demo);
                }
                return;
            }
        }
        next(&mut demo);
    }
}

fn next(demo: &mut Demo) {
    demo.index += 1;
    demo.elapsed = 0.0;
    demo.shot_taken = false;
}

/// Saves every other frame (30 fps of 60 fps virtual time) as PNG when recording, and takes the
/// script's screenshots on the frames in between.
fn record_frames(mut demo: ResMut<Demo>, view: Res<crate::DemoView>, mut commands: Commands) {
    demo.frame += 1;
    let shot = || view.0.clone().map_or_else(Screenshot::primary_window, Screenshot::image);
    if let Some(dir) = demo.record.clone()
        && demo.frame % 2 == 0
    {
        let path = dir.join(format!("f_{:05}.png", demo.frame / 2));
        commands.spawn(shot()).observe(save_to_disk(path));
    } else if let Some(path) = demo.pending_shot.take() {
        commands.spawn(shot()).observe(save_to_disk(path));
    }
}
