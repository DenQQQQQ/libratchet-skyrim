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
    Hold { secs: f32, dir: Vec2, jump: bool, dash: bool },
    Teleport(Vec3),
    ClearTrail,
    Scale(f32),
    /// Follow-camera angles.
    Follow { yaw: f32, pitch: f32, distance: f32 },
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
}

pub fn add(app: &mut App, out: PathBuf, record: Option<PathBuf>) {
    app.insert_resource(Demo { acts: script(), index: 0, elapsed: 0.0, shot_taken: false, out, record, frame: 0, finished_for: 0.0 })
        .add_systems(FixedUpdate, run_script.before(simulate))
        .add_systems(Update, record_frames);
}

fn hold(secs: f32, dir: Vec2, jump: bool) -> Act {
    Act::Hold { secs, dir, jump, dash: false }
}

fn dash(dir: Vec2) -> Act {
    Act::Hold { secs: 2.0 * TICK, dir, jump: false, dash: true }
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

/// Close follow-camera shots of the character in each move (front-right view while running +X).
fn close_ups() -> Vec<Act> {
    let x = Vec2::X;
    vec![
        Act::ClearTrail,
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
        Act::Follow { yaw: -2.0, pitch: 0.3, distance: 3.4 },
        hold(0.25, x, true),
        Act::Shot("10b_close_glide_back"),
        Act::Follow { yaw: 1.2, pitch: 0.12, distance: 3.4 },
        hold(0.6, x, false),
        hold(0.5, x, false),
        dash(x),
        hold(0.1, x, false),
        Act::Shot("11_close_dash"),
        hold(0.8, x, false),
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
    acts
}

#[allow(clippy::too_many_arguments)]
fn run_script(
    time: Res<Time>,
    mut demo: ResMut<Demo>,
    mut scripted: ResMut<ScriptedInput>,
    mut rig: ResMut<CameraRig>,
    mut settings: ResMut<Settings>,
    mut players: Query<&mut Player>,
    mut trail: ResMut<Trail>,
    mut commands: Commands,
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
            Act::Hold { secs, dir, jump, dash } => {
                scripted.0 = Some(MoveInput { move_dir: dir, jump, dash });
                demo.elapsed += dt;
                if demo.elapsed >= secs - 1e-5 {
                    next(&mut demo);
                }
                return;
            }
            Act::Shot(name) => {
                let path = demo.out.join(format!("{name}.png"));
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            }
            Act::Wide { name, focus, yaw, pitch, distance, secs } => {
                scripted.0 = Some(MoveInput::default());
                rig.fixed = Some((focus, yaw, pitch, distance));
                demo.elapsed += dt;
                if !demo.shot_taken && demo.elapsed >= 0.3 {
                    let path = demo.out.join(format!("{name}.png"));
                    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
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

/// Saves every other frame (30 fps of 60 fps virtual time) as PNG.
fn record_frames(mut demo: ResMut<Demo>, mut commands: Commands) {
    let Some(dir) = demo.record.clone() else { return };
    demo.frame += 1;
    if demo.frame % 2 == 0 {
        let path = dir.join(format!("f_{:05}.png", demo.frame / 2));
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}
