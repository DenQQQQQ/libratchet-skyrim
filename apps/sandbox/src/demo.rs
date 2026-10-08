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
    Hold { secs: f32, dir: Vec2, jump: bool, dash: bool, melee: bool, crouch: bool, swing: bool },
    /// Throw the wrench (straight ahead).
    Throw,
    /// Pick a weapon.
    Slot(crate::weapons::Slot),
    /// Gun controls from here on: trigger held, aiming.
    Gun { trigger: bool, aim: bool },
    Teleport(Vec3),
    ClearTrail,
    /// Draw the trail, and the stick's direction under him, even in a clean recording (from
    /// here on); `flat`: on the floor under where he went, to read a path from above.
    Trail { shown: bool, flat: bool },
    /// A line of text at the bottom of the picture from here on ("" for none).
    Caption(&'static str),
    Scale(f32),
    /// Follow-camera angles (also where guns aim, and back from `Watch`).
    Follow { yaw: f32, pitch: f32, distance: f32 },
    /// Close-up framing (the camera looks at his chest, not the game's pivot above him).
    CloseUp(bool),
    /// Watch from a fixed camera from here on, without stopping.
    Watch { focus: Vec3, yaw: f32, pitch: f32, distance: f32 },
    /// Film him from these angles from here on, while the follow camera he sees and aims by
    /// keeps going behind it (back with `Follow`).
    Film { yaw: f32, pitch: f32, distance: f32 },
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
        Some("sounds") => [vec![hold(1.5, Vec2::ZERO, false)], sound_tour()].concat(),
        Some("carry") => [vec![hold(1.5, Vec2::ZERO, false)], carry()].concat(),
        Some("dashjump") => [vec![hold(1.5, Vec2::ZERO, false)], dash_jumps()].concat(),
        Some("throwswitch") => [vec![hold(1.5, Vec2::ZERO, false)], throw_switch()].concat(),
        Some("strafejump") => [vec![hold(1.5, Vec2::ZERO, false)], strafe_jumps()].concat(),
        Some("walljump") => [vec![hold(1.5, Vec2::ZERO, false)], wall_jumps()].concat(),
        Some("ledges") => [vec![hold(1.5, Vec2::ZERO, false)], ledges()].concat(),
        Some("swingshot") => [vec![hold(1.5, Vec2::ZERO, false)], swingshot()].concat(),
        Some("turns") => [vec![hold(1.5, Vec2::ZERO, false)], turns()].concat(),
        _ => script(),
    };
    if let Some(dir) = &record {
        let log = dir.join("sounds.log");
        std::fs::remove_file(&log).ok();
        app.insert_resource(crate::sound::SoundLog(log));
    }
    app.insert_resource(Demo { acts, index: 0, elapsed: 0.0, shot_taken: false, out, record, frame: 0, finished_for: 0.0, pending_shot: None })
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, run_script.before(simulate))
        .add_systems(Update, (record_frames, draw_stick.after(crate::update_visuals)));
}

/// The caption line (`Act::Caption`).
#[derive(Component)]
struct Caption;

fn setup(mut commands: Commands, mut gizmos: ResMut<GizmoConfigStore>) {
    // Lines a little thicker than at the desk, to read in a video.
    gizmos.config_mut::<DefaultGizmoConfigGroup>().0.line.width = 3.0;
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(28.0),
            left: px(0.0),
            right: px(0.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_child((
            Caption,
            Text::new(""),
            TextFont::from_font_size(24.0),
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
            TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) },
        ));
}

/// With the trail shown, the stick's direction from under him (black): what the script
/// pushes.
fn draw_stick(trail: Res<Trail>, scripted: Res<ScriptedInput>, players: Query<&Transform, With<Player>>, mut gizmos: Gizmos) {
    if !trail.shown {
        return;
    }
    let (Some(input), Ok(tf)) = (scripted.0.as_ref(), players.single()) else { return };
    let dir = input.move_dir;
    if dir.length() < 1e-3 {
        return;
    }
    let from = tf.translation + Vec3::Y * 0.08;
    let to = from + Vec3::new(dir.x, 0.0, dir.y) * 1.6 * tf.scale.x;
    gizmos.arrow(from, to, Color::srgb(0.06, 0.06, 0.1)).with_tip_length(0.35 * tf.scale.x);
}

fn hold(secs: f32, dir: Vec2, jump: bool) -> Act {
    Act::Hold { secs, dir, jump, dash: false, melee: false, crouch: false, swing: false }
}

fn dash(dir: Vec2) -> Act {
    Act::Hold { secs: 2.0 * TICK, dir, jump: false, dash: true, melee: false, crouch: false, swing: false }
}

/// A melee press (a swing on the ground, a Hyper-Strike in the air).
fn swing() -> Act {
    Act::Hold { secs: 2.0 * TICK, dir: Vec2::ZERO, jump: false, dash: false, melee: true, crouch: false, swing: false }
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
        Act::Slot(Slot::BurstPistol),
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
        Act::Follow { yaw: 0.9, pitch: 0.2, distance: 9.0 },
    ]);
    acts
}

/// The weapon in hand: the pistol out standing, walking, running and in a jump; the whole wrench
/// back after the combo (the third hit shrinks its back hammer away); melee presses doing nothing
/// while it's thrown; the Shatterbomb's glove on his right hand; dashing while aiming.
fn carry() -> Vec<Act> {
    use crate::weapons::Slot;
    let arena = spots::ARENA;
    let x = Vec2::X;
    let start = arena + Vec3::new(-1.5, 0.0, 0.0);
    let lane = arena + Vec3::new(-8.0, 0.0, 6.0);
    let mut acts = vec![
        Act::ClearTrail,
        Act::CloseUp(true),
        Act::Slot(Slot::BurstPistol),
        Act::Teleport(lane),
        hold(0.12, x, false),
        Act::Follow { yaw: 0.9, pitch: 0.15, distance: 4.0 },
        hold(1.2, Vec2::ZERO, false),
        Act::Shot("50_idle"),
        hold(1.0, x * 0.3, false),
        Act::Shot("51_walk"),
        hold(0.4, x * 0.3, false),
        hold(1.0, x, false),
        Act::Shot("52_run"),
        hold(0.25, x, true),
        hold(0.1, x, false),
        Act::Shot("53_jump"),
        hold(0.8, x, false),
        hold(1.0, Vec2::ZERO, false),
        // The combo (mashed), then the wrench as it rests: whole again.
        Act::Teleport(start),
        hold(0.12, x, false),
        Act::Follow { yaw: 0.9, pitch: 0.2, distance: 4.0 },
        hold(0.6, Vec2::ZERO, false),
    ];
    for i in 0..9 {
        acts.push(swing());
        acts.push(hold(0.1 - 2.0 * TICK, Vec2::ZERO, false));
        if i == 6 {
            acts.push(Act::Shot("54_third_hit"));
        }
    }
    acts.extend([
        hold(1.0, Vec2::ZERO, false),
        Act::Shot("55_after_the_combo"),
        hold(0.6, Vec2::ZERO, false),
        // Thrown: melee presses while it's out do nothing.
        Act::Throw,
        hold(0.3, Vec2::ZERO, false),
        swing(),
        hold(0.2, Vec2::ZERO, false),
        Act::Shot("56_melee_while_thrown"),
        swing(),
        hold(1.6, Vec2::ZERO, false),
        // The Shatterbomb: its glove goes on his right hand.
        Act::Slot(Slot::Shatterbomb),
        Act::Teleport(lane),
        hold(0.12, x, false),
        Act::Follow { yaw: 0.9, pitch: 0.15, distance: 4.0 },
        hold(1.2, Vec2::ZERO, false),
        Act::Shot("57_glove_idle"),
        Act::Wide { name: "57b_glove_close", focus: lane + Vec3::new(0.1, 0.75, 0.0), yaw: 1.7, pitch: 0.1, distance: 1.4, secs: 0.8 },
        Act::Follow { yaw: 0.9, pitch: 0.15, distance: 4.0 },
        hold(1.0, x, false),
        Act::Shot("58_glove_run"),
        hold(0.6, x, false),
        hold(0.8, Vec2::ZERO, false),
        // Dashing while aiming: he keeps facing the aim, dashing to his right, then back (the
        // game's directional dashes); the pistol is put away for each.
        Act::Slot(Slot::BurstPistol),
        Act::Teleport(lane + Vec3::new(4.0, 0.0, -3.0)),
        hold(0.12, x, false),
        Act::Follow { yaw: -std::f32::consts::FRAC_PI_2, pitch: 0.15, distance: 5.0 },
        Act::Gun { trigger: false, aim: true },
        hold(0.6, Vec2::ZERO, false),
        dash(Vec2::Y),
        hold(0.15, Vec2::ZERO, false),
        Act::Shot("5a_dash_right_aiming"),
        hold(0.9, Vec2::ZERO, false),
        dash(-x),
        hold(0.15, Vec2::ZERO, false),
        Act::Shot("5b_dash_back_aiming"),
        hold(0.9, Vec2::ZERO, false),
        Act::Gun { trigger: false, aim: false },
        hold(0.6, Vec2::ZERO, false),
    ]);
    acts
}

/// A running jump beside jumps out of a dash (0.1 and 0.2 s into it), in three lanes watched
/// from the side: a jump keeps the dash's speed for one tick, then goes at the jump's speeds, so
/// all three land about as far. Run it without `--clean` to see the trails and the HUD.
fn dash_jumps() -> Vec<Act> {
    let x = Vec2::X;
    let lane = spots::DEMO_LANE;
    let mut acts = vec![
        Act::ClearTrail,
        Act::Watch { focus: lane + Vec3::new(7.0, 0.8, 3.0), yaw: 0.0, pitch: 0.35, distance: 17.0 },
    ];
    for (i, dash_for) in [None, Some(0.1), Some(0.2)].into_iter().enumerate() {
        acts.extend([Act::Teleport(lane + Vec3::new(0.0, 0.0, 3.0 * i as f32)), hold(0.12, x, false), hold(0.4, Vec2::ZERO, false), hold(1.0, x, false)]);
        if let Some(secs) = dash_for {
            acts.extend([dash(x), hold(secs - 2.0 * TICK, x, false)]);
        }
        acts.extend([hold(0.05, x, true), hold(1.0, x, false), hold(0.5, Vec2::ZERO, false)]);
    }
    acts.push(Act::Shot("60_dash_jumps"));
    acts.push(hold(0.5, Vec2::ZERO, false));
    acts
}

/// Switching weapons around a wrench throw, as in the game: with the pistol out, a throw puts it
/// away until the wrench is back; picking another weapon while the wrench is out does nothing;
/// after the catch, a switch works and draws the new weapon.
fn throw_switch() -> Vec<Act> {
    use crate::weapons::Slot;
    let x = Vec2::X;
    let lane = spots::ARENA + Vec3::new(-8.0, 0.0, 6.0);
    vec![
        Act::ClearTrail,
        Act::CloseUp(true),
        Act::Slot(Slot::BurstPistol),
        Act::Teleport(lane),
        hold(0.12, x, false),
        Act::Follow { yaw: 0.9, pitch: 0.15, distance: 4.0 },
        hold(0.8, Vec2::ZERO, false),
        Act::Throw,
        hold(0.3, Vec2::ZERO, false),
        Act::Slot(Slot::Shatterbomb),
        hold(0.4, Vec2::ZERO, false),
        Act::Shot("62_no_switch_while_thrown"),
        hold(1.5, Vec2::ZERO, false),
        Act::Shot("63_pistol_back_after_the_catch"),
        hold(0.6, Vec2::ZERO, false),
        Act::Slot(Slot::Shatterbomb),
        hold(0.3, Vec2::ZERO, false),
        Act::Shot("64_switch_after_the_catch"),
        hold(1.4, Vec2::ZERO, false),
        Act::Slot(Slot::BurstPistol),
        hold(1.0, Vec2::ZERO, false),
    ]
}

/// Jumping while aiming: the game's strafe jumps by direction (side flips to his right and left, a
/// back flip, ahead) and standing, a sideways double jump; then a glide off the tower under the
/// heli-pack (its intro, the swaying loop, leaning into a turn).
fn strafe_jumps() -> Vec<Act> {
    use crate::weapons::Slot;
    let x = Vec2::X;
    // Facing +X, his right is +Z.
    let right = Vec2::Y;
    let lane = spots::ARENA + Vec3::new(-8.0, 0.0, 10.0);
    let behind = -std::f32::consts::FRAC_PI_2;
    let mut acts = vec![
        Act::ClearTrail,
        Act::Slot(Slot::BurstPistol),
        Act::Teleport(lane),
        hold(0.12, x, false),
        Act::Follow { yaw: behind + 0.7, pitch: 0.15, distance: 5.5 },
        Act::Gun { trigger: false, aim: true },
        hold(0.6, Vec2::ZERO, false),
    ];
    for (name, dir) in [
        ("65_strafe_jump_right", right),
        ("66_strafe_jump_left", -right),
        ("67_strafe_jump_back", -x),
        ("68_strafe_jump_ahead", x),
        ("69_strafe_jump_standing", Vec2::ZERO),
    ] {
        acts.extend([hold(0.3, dir, false), hold(0.05, dir, true), hold(0.2, dir, false), Act::Shot(name), hold(0.5, dir, false), hold(0.5, Vec2::ZERO, false)]);
    }
    acts.extend([
        hold(0.3, right, false),
        hold(0.05, right, true),
        hold(0.3, right, false),
        hold(0.05, right, true),
        hold(0.15, right, false),
        Act::Shot("6a_strafe_double_jump"),
        hold(0.8, right, false),
        hold(0.5, Vec2::ZERO, false),
        Act::Gun { trigger: false, aim: false },
        // A long glide from high up (a mid-air jump, then holding it), turning partway.
        Act::Teleport(lane + Vec3::new(0.0, 40.0, 0.0)),
        Act::Follow { yaw: -1.9, pitch: 0.15, distance: 4.0 },
        hold(0.3, x, false),
        hold(1.2, x, true),
        Act::Shot("6b_glide_intro"),
        hold(2.6, x, true),
        Act::Shot("6c_glide_loop"),
        hold(1.0, x, true),
    ]);
    acts.extend((1..=16).map(|i| hold(0.05, Vec2::from_angle(0.1 * i as f32).rotate(x), true)));
    acts.extend([Act::Shot("6d_glide_lean"), hold(1.5, Vec2::from_angle(1.6).rotate(x), true), hold(3.0, x, false)]);
    acts
}

/// Ledges: falling short of a platform and catching its lip, then climbing up; grabbing a
/// placed ledge on the way down, going up to the next one, and jumping up over it; letting go
/// of one.
fn ledges() -> Vec<Act> {
    let l = spots::LEDGES;
    let x = Vec2::X;
    let side = Act::Follow { yaw: std::f32::consts::PI, pitch: 0.2, distance: 7.0 };
    vec![
        Act::ClearTrail,
        Act::Caption("Off a block, short of a platform: he catches its lip and hangs on"),
        // Off a 3 m block at a 5 m platform 3 m away: he falls short, catches the lip, hangs
        // on, and a jump climbs up.
        Act::Teleport(l + Vec3::new(-8.6, 3.0, 0.0)),
        hold(0.12, x, false),
        side.clone(),
        hold(0.5, Vec2::ZERO, false),
        hold(0.45, x, false),
        hold(0.2, x, true),
        hold(0.3, x, false),
        hold(0.25, Vec2::ZERO, false),
        Act::Shot("80_ledge_catch"),
        hold(0.8, Vec2::ZERO, false),
        Act::Shot("81_ledge_hang"),
        Act::Caption("Jump: he climbs up"),
        hold(0.05, Vec2::ZERO, true),
        hold(0.3, Vec2::ZERO, false),
        Act::Shot("82_ledge_climb"),
        hold(1.2, Vec2::ZERO, false),
        // The wall's placed ledges (yellow): up at the one at 3 m, grabbed on the way down; a
        // jump to the one along the top, and the ledge jump up over it.
        Act::Caption("Placed ledges (yellow): a jump, and he grabs one on the way down"),
        Act::Teleport(l + Vec3::new(5.0, 0.0, 0.0)),
        hold(0.12, x, false),
        side.clone(),
        hold(0.5, Vec2::ZERO, false),
        hold(0.25, Vec2::ZERO, true),
        hold(0.6, Vec2::ZERO, false),
        Act::Shot("83_ledge_grab"),
        hold(0.5, Vec2::ZERO, false),
        Act::Caption("Jump: up to the next one"),
        hold(0.05, Vec2::ZERO, true),
        hold(0.4, Vec2::ZERO, false),
        Act::Shot("84_ledge_up"),
        hold(0.8, Vec2::ZERO, false),
        Act::Caption("Jump at the top: the ledge jump, up over it"),
        hold(0.05, Vec2::ZERO, true),
        hold(0.3, Vec2::ZERO, false),
        Act::Shot("85_ledge_jump"),
        hold(1.2, Vec2::ZERO, false),
        // Letting go of the lower one: the turn, then the fall.
        Act::Caption("Hanging on the lower ledge..."),
        Act::Teleport(l + Vec3::new(5.0, 0.0, 0.0)),
        hold(0.12, x, false),
        side,
        hold(0.5, Vec2::ZERO, false),
        hold(0.25, Vec2::ZERO, true),
        hold(1.1, Vec2::ZERO, false),
        Act::Caption("...crouch lets go"),
        crouch(0.05),
        hold(0.25, Vec2::ZERO, false),
        Act::Shot("86_ledge_drop"),
        hold(1.2, Vec2::ZERO, false),
        Act::Caption(""),
    ]
}

/// The Swingshot: off a platform onto three targets in a row, letting go near the top of each
/// swing, onto a landing platform; then the Hookshot up onto a block; then swinging on the first
/// target for a while.
fn swingshot() -> Vec<Act> {
    let s = spots::SWINGS;
    let z = Vec2::Y;
    let mut acts = vec![];
    // Twice: through his camera (the game's swing camera turns it along each swing), then filmed
    // from the side while that camera, which he picks targets by, keeps going behind him.
    for pass in 0..2 {
        acts.extend([
            Act::ClearTrail,
            Act::Teleport(s + Vec3::new(0.0, 2.0, -3.5)),
            hold(0.12, z, false),
            // Looking along the course, level, as a player would to find the first target.
            Act::Follow { yaw: std::f32::consts::PI, pitch: 0.0, distance: 4.65 },
        ]);
        acts.push(Act::Caption(if pass == 0 {
            "Swingshot, through his camera: three targets, off each with a jump"
        } else {
            "The same run from the side"
        }));
        if pass == 1 {
            acts.push(Act::Film { yaw: -std::f32::consts::FRAC_PI_2, pitch: 0.12, distance: 13.0 });
        }
        acts.extend([
            hold(0.6, Vec2::ZERO, false),
            // Run off the platform, jump, and fire at the first target on the way up.
            hold(0.45, z, false),
            hold(0.15, z, true),
        ]);
        for (i, swing) in SWING_HOLDS.into_iter().enumerate() {
            acts.push(swinging(swing, Vec2::ZERO));
            if pass == 0 && i == 0 {
                acts.push(Act::Shot("90_swing"));
            }
            // Off with a jump, then straight at the next target.
            acts.extend([hold(0.05, z, true), hold(0.12, z, false)]);
            if pass == 0 && i == 0 {
                acts.push(Act::Shot("91_swing_jump"));
            }
        }
        // Down onto the landing platform.
        acts.extend([hold(0.9, z, false), hold(0.5, Vec2::ZERO, false)]);
        if pass == 0 {
            // A shot takes the frame the next acts run in: hold still for it.
            acts.extend([Act::Shot("92_swing_landed"), hold(0.1, Vec2::ZERO, false)]);
        }
        // The Hookshot up onto the block.
        acts.push(Act::Caption("Hookshot: reeled up onto the block"));
        acts.push(swinging(1.2, Vec2::ZERO));
        if pass == 0 {
            acts.push(Act::Shot("93_hookshot"));
        }
        acts.push(hold(1.0, Vec2::ZERO, false));
        if pass == 0 {
            acts.extend([Act::Shot("94_hookshot_top"), hold(0.5, Vec2::ZERO, false)]);
        }
    }
    // Then on the first target for a while, from the side: the intro through the first swing,
    // then the loop's clips blending as he swings back and forth; off with a jump on the second
    // swing ahead, and the cable flies back.
    acts.extend([
        Act::ClearTrail,
        Act::Teleport(s + Vec3::new(0.0, 2.0, -3.5)),
        hold(0.12, z, false),
        Act::Follow { yaw: std::f32::consts::PI, pitch: 0.0, distance: 4.65 },
        Act::Caption("Held on one target: the intro, then the loop as he swings back and forth"),
        // Closer, and from a little below him so the target stays in the picture.
        Act::Film { yaw: -std::f32::consts::FRAC_PI_2, pitch: -0.15, distance: 10.0 },
        hold(0.6, Vec2::ZERO, false),
        hold(0.45, z, false),
        hold(0.15, z, true),
        swinging(2.4, Vec2::ZERO),
        Act::Shot("95_swing_loop"),
        swinging(2.1, Vec2::ZERO),
        hold(0.05, z, true),
        hold(1.5, Vec2::ZERO, false),
        Act::Caption(""),
    ]);
    acts
}

/// Turning in the air and in a dash, filmed from above with the trail (flat on the floor) and
/// the stick (black arrow) drawn: the same run along +X three times, the stick swung to −Z
/// (up the screen) partway into a single jump, a double jump and a dash, then the whole path
/// from higher up.
/// A single jump keeps almost none of its momentum, so where he goes and where he faces swing
/// round at once; a double jump keeps more, so its path rounds the corner and he comes round
/// to face it lazily; a dash bends toward the stick at up to 180°/s.
fn turns() -> Vec<Act> {
    let x = Vec2::X;
    let up = Vec2::NEG_Y;
    let lane = spots::DEMO_LANE;
    let start = |caption: &'static str| {
        vec![
            Act::Caption(caption),
            Act::Teleport(lane),
            Act::ClearTrail,
            hold(0.12, x, false),
            Act::Follow { yaw: -std::f32::consts::FRAC_PI_2, pitch: 0.2, distance: 5.0 },
            Act::Film { yaw: 0.0, pitch: 0.95, distance: 10.0 },
            hold(0.5, Vec2::ZERO, false),
            hold(0.8, x, false),
        ]
    };
    let overview = |shot: &'static str| {
        [
            hold(0.6, Vec2::ZERO, false),
            Act::Watch { focus: lane + Vec3::new(4.5, 0.0, -2.2), yaw: 0.0, pitch: 1.2, distance: 13.0 },
            hold(0.3, Vec2::ZERO, false),
            Act::Shot(shot),
            hold(1.5, Vec2::ZERO, false),
        ]
    };
    let mut acts = vec![Act::Trail { shown: true, flat: true }];
    acts.extend(start("Single jump, stick swung up the screen 0.2 s in: he turns at once"));
    acts.extend([hold(0.2, x, true), hold(0.6, up, false)]);
    acts.extend(overview("100_turn_single_jump"));
    acts.extend(start("Double jump, the same swing: it keeps more momentum, so he comes round lazily"));
    acts.extend([hold(0.2, x, true), hold(0.1, x, false), hold(0.2, x, true), hold(0.6, up, false)]);
    acts.extend(overview("101_turn_double_jump"));
    acts.extend(start("Dash, stick swung up 0.1 s in: the dash bends toward it, at up to 180 degrees a second"));
    acts.extend([dash(x), hold(0.1 - 2.0 * TICK, x, false), hold(0.6, up, false)]);
    acts.extend(overview("102_dash_bends"));
    acts.extend([Act::Trail { shown: false, flat: false }, Act::Caption("")]);
    acts
}

/// How long the swingshot demo holds on to each target before jumping off.
const SWING_HOLDS: [f32; 3] = [0.85, 0.85, 0.85];

fn swinging(secs: f32, dir: Vec2) -> Act {
    Act::Hold { secs, dir, jump: false, dash: false, melee: false, crouch: false, swing: true }
}

fn crouch(secs: f32) -> Act {
    Act::Hold { secs, dir: Vec2::ZERO, jump: false, dash: false, melee: false, crouch: true, swing: false }
}

/// Wall jumps up the shaft (two walls that take wall jumps, 3.4 m apart, 12.5 m tall): onto
/// the first wall from a running jump, then back and forth up to the ledge on top; then the
/// plain walls beside it, which take one wall jump each. Filmed from the shaft's open side.
fn wall_jumps() -> Vec<Act> {
    let shaft = spots::WALL_SHAFT;
    // The walls face each other across Z.
    let z = Vec2::Y;
    let mut acts = vec![
        Act::ClearTrail,
        Act::Caption("Wall-jump shaft: a running jump onto one wall, then back and forth up it"),
        // He runs into the shaft from its open +X end, the camera behind him and off to the side.
        // The shaft's wall-jump zone turns it to look straight along the shaft, a little down,
        // and pulls it back.
        Act::Teleport(shaft + Vec3::new(7.0, 0.0, -1.2)),
        hold(0.12, -Vec2::X, false),
        Act::Follow { yaw: std::f32::consts::FRAC_PI_2 + 0.6, pitch: 0.35, distance: 4.65 },
        hold(0.4, Vec2::ZERO, false),
        hold(1.15, -Vec2::X, false),
        hold(0.6, Vec2::ZERO, false),
        hold(0.12, z, false),
        hold(0.6, Vec2::ZERO, false),
        hold(0.1, z, false),
        hold(0.25, z, true),
        hold(0.15, z, false),
        Act::Shot("71_wall_stick"),
    ];
    // Each jump presses about 0.15 s after landing on the wall (before it starts sliding). At
    // 5 m/s he reaches the other wall 0.55 s after the jump, just past the jump-off's 0.5 s
    // block, so the next press comes 0.7 s after this one, about 1.9 m higher. The sixth
    // clears the top onto the ledge.
    for i in 0..6 {
        acts.extend([hold(0.15, Vec2::ZERO, false), hold(0.05, Vec2::ZERO, true), hold(0.3, Vec2::ZERO, false)]);
        if i == 1 {
            acts.push(Act::Shot("72_wall_jump"));
        }
        acts.push(hold(0.2, Vec2::ZERO, false));
    }
    acts.extend([
        // On the ledge.
        Act::Caption("...and out over the top"),
        hold(1.0, Vec2::ZERO, false),
        Act::Shot("73_wall_shaft_top"),
        hold(0.6, Vec2::ZERO, false),
        // Left on the wall: a double jump onto it, held until he slides down to a metre above
        // the floor and lets go.
        Act::Caption("Left on the wall: he slides down and lets go a metre above the floor"),
        Act::Teleport(shaft + Vec3::new(0.0, 0.0, -1.2)),
        hold(0.12, z, false),
        hold(0.5, Vec2::ZERO, false),
        hold(0.1, z, false),
        hold(0.15, z, true),
        hold(0.05, z, false),
        hold(0.15, z, true),
        hold(0.45, Vec2::ZERO, false),
        Act::Shot("73b_wall_slide"),
        hold(1.3, Vec2::ZERO, false),
        // Plain walls (grey): he sticks to one only once per air time, so between two of them he
        // jumps across, bumps into the other and drops.
        Act::Caption("Plain walls (grey) take one wall jump per air time: across, a bump, a drop"),
        Act::Teleport(shaft + Vec3::new(-13.3, 0.0, 0.0)),
        hold(0.12, Vec2::X, false),
        Act::Watch { focus: shaft + Vec3::new(-12.1, 2.5, 0.0), yaw: 0.0, pitch: 0.12, distance: 11.0 },
        hold(0.5, Vec2::ZERO, false),
        hold(0.1, Vec2::X, false),
        hold(0.25, Vec2::X, true),
        hold(0.15, Vec2::X, false),
        Act::Shot("74_plain_wall"),
        hold(0.15, Vec2::ZERO, false),
        hold(0.05, Vec2::ZERO, true),
        hold(0.5, Vec2::ZERO, false),
        Act::Shot("75_plain_wall_no_second_stick"),
        hold(1.3, Vec2::ZERO, false),
        // Pushing the stick into a plain wall jumps up it instead, with his double jump.
        Act::Caption("Stick pushed into a plain wall: up it with the double jump"),
        hold(0.1, Vec2::X, false),
        hold(0.25, Vec2::X, true),
        hold(0.35, Vec2::X, false),
        hold(0.05, Vec2::X, true),
        hold(0.3, Vec2::X, false),
        Act::Shot("76_plain_wall_jump_up"),
        hold(1.5, Vec2::ZERO, false),
        Act::Caption(""),
    ]);
    acts
}

/// The moves that make the game's sounds: walking and running (steps, foley), jumps and a
/// landing, a glide off the tower (the heli-pack unfolding, spinning, folding away), dashes,
/// then the combat section.
fn sound_tour() -> Vec<Act> {
    let x = Vec2::X;
    let mut acts = vec![
        Act::Teleport(spots::DEMO_LANE),
        Act::Follow { yaw: 1.2, pitch: 0.15, distance: 5.0 },
        hold(0.5, Vec2::ZERO, false),
        hold(1.6, x * 0.45, false),
        hold(1.6, x, false),
        hold(0.22, x, true),
        hold(0.3, x, false),
        hold(0.6, x, false),
        hold(0.22, x, true),
        hold(0.04, x, false),
        hold(0.2, x, true),
        hold(1.0, x, false),
        hold(0.6, Vec2::ZERO, false),
        // Off the tower: jump, hold to glide, land.
        Act::Teleport(spots::TOWER_TOP + Vec3::new(-1.5, 0.0, 0.0)),
        Act::Follow { yaw: -1.9, pitch: 0.2, distance: 4.0 },
        hold(0.6, Vec2::ZERO, false),
        hold(0.45, x, false),
        hold(0.22, x, true),
        hold(0.04, x, false),
        hold(2.6, x, true),
        hold(1.5, x, false),
        hold(0.6, Vec2::ZERO, false),
        Act::Teleport(spots::DEMO_LANE),
        Act::Follow { yaw: 1.2, pitch: 0.15, distance: 6.0 },
    ];
    acts.extend(dashes());
    acts.extend(combat());
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

/// The Burst Pistol and the Shatterbomb on the dummies in the arena (Ratchet facing +X, the
/// camera behind him, aiming where it looks).
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
        Act::Slot(Slot::BurstPistol),
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
    level: Res<crate::level::Level>,
    mut captions: Query<&mut Text, With<Caption>>,
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
            Act::Teleport(at) => respawn(&mut p, at, &level, settings.scale),
            Act::ClearTrail => trail.clear(),
            Act::Trail { shown, flat } => {
                trail.shown = shown;
                trail.flat = flat;
            }
            Act::Caption(line) => {
                for mut text in &mut captions {
                    text.0 = line.to_string();
                }
            }
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
                rig.film = None;
                rig.focus = p.feet;
            }
            Act::Film { yaw, pitch, distance } => {
                rig.film = Some((yaw, pitch, distance));
                rig.film_focus = p.feet;
            }
            Act::Throw => throw.0 = true,
            Act::Slot(slot) => {
                if let Some(a) = arsenal.as_mut() {
                    a.requested = Some(slot);
                }
            }
            Act::Gun { trigger, aim } => *gun = crate::weapons::ScriptedArsenal { active: true, trigger, aim, target: gun.target },
            Act::AimAt(at) => gun.target = at,
            Act::CloseUp(on) => rig.close_up = on,
            Act::Watch { focus, yaw, pitch, distance } => rig.fixed = Some((focus, yaw, pitch, distance)),
            Act::Hold { secs, dir, jump, dash, melee, crouch, swing } => {
                scripted.0 = Some(MoveInput { move_dir: dir, jump, dash, melee, crouch, swingshot: swing, ..Default::default() });
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
        && demo.frame.is_multiple_of(2)
    {
        let path = dir.join(format!("f_{:05}.png", demo.frame / 2));
        commands.spawn(shot()).observe(save_to_disk(path));
    } else if let Some(path) = demo.pending_shot.take() {
        commands.spawn(shot()).observe(save_to_disk(path));
    }
}
