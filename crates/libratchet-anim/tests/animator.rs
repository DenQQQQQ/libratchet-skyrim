//! Drives the animator with libratchet's sim as the sandbox does: the sim ticks at 120 Hz (on a
//! floor, with nothing under him, against a wall or on a Swingshot target), the animator runs
//! once a 60 Hz frame. The checks follow the sandbox's rules (`apps/sandbox/src/ratchet_model.rs`).

use libratchet::glam::{Vec2, Vec3};
use libratchet::{Contact, Event, Input, Ledge, MoveState, RayHit, Ratchet, SwingKind, SwingTarget, Tuning, WallContact, World};
use libratchet_anim::{AnimInput, Animator, ClipInfo, Layer, LayerKind, Slot, clip_names};

const TICK: f32 = 1.0 / 120.0;
const FRAME: f32 = 2.0 * TICK;
const AHEAD: Vec2 = Vec2::new(0.0, 1.0);
/// His right, facing ahead (+Z).
const RIGHT: Vec2 = Vec2::new(-1.0, 0.0);
const SWING_LOOP: [&str; 5] = ["swing_forward_fast", "swing_forward_slow", "swing_idle", "swing_backward_slow", "swing_backward_fast"];

/// Every clip the animator looks for, at made-up lengths (not the game's): a second each, but
/// for the ones the checks time. The runs and walks move their root; the swing jump has a
/// JumpPeak.
fn clips() -> Vec<ClipInfo> {
    clip_names()
        .into_iter()
        .map(|name| {
            let (seconds, root_distance) = match name {
                // Authored at 5 m/s.
                "run" | "wrench_run" | "run_lean_left" | "run_lean_right" | "wrench_run_lean_left" | "wrench_run_lean_right" => (1.2, 6.0),
                // At 1.2 m/s.
                "walk" | "wrench_walk" => (4.0, 4.8),
                "idle_look" => (5.0, 0.0),
                "idle_shoe" => (4.0, 0.0),
                "glide_intro_pistol" | "glide_intro_wrench" | "glide_intro_glove" => (2.0, 0.0),
                "fire_pistol_additive" | "throw_catch" => (0.4, 0.0),
                "throw_additive" => (2.0, 0.0),
                "glove_hand" | "glide_lean_additive" => (0.5, 0.0),
                name if name.starts_with("aim_") => (0.8, 0.0),
                _ => (1.0, 0.0),
            };
            ClipInfo { name: name.to_string(), seconds, root_distance, jump_peak: (name == "swing_jump").then_some(0.5) }
        })
        .collect()
}

/// The level: a floor at y = 0 (or none), maybe a wall-jump wall at this x facing -X, and
/// Swingshot targets (all in view: no rays hit anything).
struct Level {
    floor: bool,
    wall_x: Option<f32>,
    targets: Vec<SwingTarget>,
}

impl World for Level {
    fn ledges(&self, _center: Vec3, _radius: f32, _out: &mut Vec<Ledge>) {}

    fn raycast(&self, _from: Vec3, _to: Vec3) -> Option<RayHit> {
        None
    }

    fn swing_targets(&self, _center: Vec3, _radius: f32, out: &mut Vec<SwingTarget>) {
        out.extend(self.targets.iter().copied());
    }
}

/// A minimal host: the sim in its level, the animator, and the weapons as the sandbox's arsenal
/// and wrench would give them.
struct Host {
    sim: Ratchet,
    level: Level,
    pos: Vec3,
    contact: Contact,
    /// The camera: behind him, looking ahead and a little up.
    view: Vec3,
    names: Vec<String>,
    anim: Animator,
    /// The weapons, the wrench and a respawn, as the host passes them (the frame's own values
    /// are filled in each frame).
    weapons: AnimInput,
    time: f32,
}

impl Host {
    fn new() -> Host {
        Host::on(Level { floor: true, wall_x: None, targets: vec![] }, Vec3::ZERO)
    }

    /// High up with nothing under him: he falls.
    fn falling() -> Host {
        Host::on(Level { floor: false, wall_x: None, targets: vec![] }, Vec3::new(0.0, 100.0, 0.0))
    }

    fn on(level: Level, pos: Vec3) -> Host {
        let clips = clips();
        let mut h = Host {
            sim: Ratchet::new(Tuning::default()),
            level,
            pos,
            contact: Contact::default(),
            view: Vec3::new(0.0, 0.3, 1.0).normalize(),
            names: clips.iter().map(|c| c.name.clone()).collect(),
            anim: Animator::new(&clips),
            weapons: AnimInput::default(),
            time: 0.0,
        };
        h.touch();
        h
    }

    /// What his collision finds where he is.
    fn touch(&mut self) {
        let grounded = self.level.floor && self.pos.y <= 0.0;
        if grounded {
            self.pos.y = 0.0;
        }
        let mut wall = None;
        if let Some(x) = self.level.wall_x
            && self.pos.x >= x
        {
            self.pos.x = x;
            wall = Some(WallContact { normal: Vec2::new(-1.0, 0.0), sticky: true, distance: None });
        }
        let ground_distance = self.level.floor.then_some(self.pos.y.max(0.0));
        self.contact = Contact { grounded, ground_distance, hit_ceiling: false, wall, wall_jump_zone: false, landing_ahead: false };
    }

    /// One sim tick; true if he let go of a ledge.
    fn tick(&mut self, input: &Input) -> bool {
        let view_right = Vec2::new(-self.view.z, self.view.x).normalize_or_zero();
        let input = Input { view_forward: self.view, view_right, ..*input };
        let v = self.sim.step_in(TICK, &input, &self.contact, self.pos, &self.level);
        let dropped = self.sim.events().contains(&Event::LedgeDropped);
        self.pos += v * TICK;
        self.touch();
        dropped
    }

    /// One frame: two sim ticks with `input`, then the animator.
    fn frame(&mut self, input: Input) {
        let mut dropped = false;
        for _ in 0..2 {
            dropped |= self.tick(&input);
        }
        self.time += FRAME;
        let frame = AnimInput {
            dt: FRAME,
            scale: 1.0,
            velocity: self.sim.velocity(),
            feet: self.pos,
            ground_distance: self.contact.ground_distance,
            ledge_dropped: dropped,
            ..self.weapons
        };
        self.anim.update(&self.sim, &frame);
        self.weapons.respawned = false;
        // The base layers come first, then the additive ones.
        let additive: Vec<bool> = self.anim.layers().iter().map(|l| l.kind == LayerKind::Additive).collect();
        assert!(additive.windows(2).all(|w| w[0] <= w[1]), "out of order: {:?}", self.anim.layers());
    }

    fn frames(&mut self, n: usize, input: Input) {
        for _ in 0..n {
            self.frame(input);
        }
    }

    fn clip(&self) -> &'static str {
        self.anim.clip_name().unwrap_or("")
    }

    fn index(&self, name: &str) -> usize {
        self.names.iter().position(|n| n == name).unwrap()
    }

    fn layer(&self, name: &str) -> Option<Layer> {
        self.anim.layers().iter().copied().find(|l| self.names[l.clip] == name)
    }

    fn weight(&self, name: &str) -> f32 {
        self.layer(name).map_or(0.0, |l| l.weight)
    }

    fn flat_speed(&self) -> f32 {
        Vec2::new(self.sim.velocity().x, self.sim.velocity().z).length()
    }
}

fn still() -> Input {
    Input::default()
}

fn stick(dir: Vec2) -> Input {
    Input { move_dir: dir, ..Default::default() }
}

fn jump() -> Input {
    Input { jump: true, ..Default::default() }
}

/// In aim mode, facing ahead.
fn aim(dir: Vec2, jump: bool) -> Input {
    Input { move_dir: dir, jump, aim: Some(AHEAD), aim_mode: true, ..Default::default() }
}

#[test]
fn stands_idle_then_fidgets_after_seven_seconds() {
    let mut h = Host::new();
    h.frame(still());
    assert_eq!(h.clip(), "idle");
    // Alone at full weight, its clock a frame in: started at 0, moved on before the pose.
    assert_eq!(h.anim.layers(), &[Layer { clip: h.index("idle"), time: FRAME, weight: 1.0, kind: LayerKind::Base }]);
    // The sounds read its clock before that: new, at 0.
    let clock = h.anim.sound_clock().unwrap();
    assert_eq!((clock.clip, clock.from, clock.now), (h.index("idle"), None, 0.0));
    assert!(clock.passed(0.0) && !clock.passed(0.01));
    h.frame(still());
    let clock = h.anim.sound_clock().unwrap();
    assert_eq!((clock.from, clock.now), (Some(0.0), FRAME));
    assert!(clock.passed(FRAME) && !clock.passed(0.0));
    while h.time < 6.9 {
        h.frame(still());
        assert_eq!(h.clip(), "idle");
    }
    while h.clip() == "idle" {
        h.frame(still());
        assert!(h.time < 7.1, "no fidget by {:.2} s", h.time);
    }
    assert_eq!(h.clip(), "idle_look");
    // It comes in over 0.4 s, plays once, and the idle comes back; seven seconds on, the other.
    assert!((h.weight("idle_look") - FRAME / 0.4).abs() < 1e-5, "{}", h.weight("idle_look"));
    let start = h.time;
    while h.clip() == "idle_look" {
        h.frame(still());
    }
    assert_eq!(h.clip(), "idle");
    assert!((h.time - start - 5.0).abs() < 2.5 * FRAME, "played {:.3} s", h.time - start);
    let start = h.time;
    while h.clip() == "idle" {
        h.frame(still());
    }
    assert_eq!(h.clip(), "idle_shoe");
    assert!((h.time - start - 7.0).abs() < 2.5 * FRAME, "idled {:.3} s", h.time - start);
}

#[test]
fn walks_then_runs_at_the_rate_that_keeps_the_feet_planted() {
    let mut h = Host::new();
    h.frame(still());
    let mut picks = vec![h.clip()];
    for _ in 0..60 {
        h.frame(stick(AHEAD));
        if picks.last() != Some(&h.clip()) {
            picks.push(h.clip());
            if h.clip() == "walk" {
                // The idle was alone: it fades out over 0.16 s as the walk comes in.
                assert!((h.weight("walk") - FRAME / 0.16).abs() < 1e-5 && (h.weight("idle") - (1.0 - FRAME / 0.16)).abs() < 1e-5);
            }
        }
        // The crossfading clips share the pose between them.
        let sum: f32 = h.anim.layers().iter().map(|l| l.weight).sum();
        assert!((sum - 1.0).abs() < 1e-5, "{:?}", h.anim.layers());
    }
    assert_eq!(picks, ["idle", "walk", "run"]);
    // At his top speed (5.5 m/s) the run, authored at 5 m/s, plays at 1.1x, alone: running
    // straight, no lean.
    assert!((h.flat_speed() - 5.5).abs() < 1e-3);
    let before = h.layer("run").unwrap();
    h.frame(stick(AHEAD));
    let after = h.layer("run").unwrap();
    let moved = (after.time - before.time).rem_euclid(1.2);
    assert!((moved - 1.1 * FRAME).abs() < 1e-4, "moved {moved}");
    assert_eq!(h.anim.layers(), &[after]);
    assert_eq!(after.weight, 1.0);
}

#[test]
fn turning_at_a_run_leans_into_the_turn_in_step_with_it() {
    let mut h = Host::new();
    h.frames(60, stick(AHEAD));
    // Turning to his left (+X) at a run: the lean-left clip comes in at the run's time and rate.
    h.frames(6, stick(Vec2::new(1.0, 0.3).normalize()));
    assert_eq!(h.clip(), "run");
    let (run, left) = (h.layer("run").unwrap(), h.layer("run_lean_left").unwrap());
    assert!(h.layer("run_lean_right").is_none());
    assert_eq!(left.time, run.time);
    assert!(left.weight > 0.0 && left.kind == LayerKind::Base, "{left:?}");
}

#[test]
fn jumps_falls_and_lands() {
    let mut h = Host::new();
    h.frames(5, still());
    h.frame(jump());
    assert_eq!((h.sim.state(), h.clip()), (MoveState::Jump, "jump"));
    // Let go: up and down; coming down faster than 5 m/s, the fall clip; then the landing.
    let mut picks = vec![];
    loop {
        h.frame(still());
        if picks.last() != Some(&h.clip()) {
            picks.push(h.clip());
        }
        if h.sim.state() == MoveState::Ground {
            break;
        }
        assert!(h.time < 3.0, "never landed");
    }
    assert_eq!(picks, ["jump", "fall", "land"]);
    // Standing still, it lasts 0.3 s, then the idle.
    let start = h.time;
    while h.clip() == "land" {
        h.frame(still());
    }
    assert_eq!(h.clip(), "idle");
    assert!((h.time - start - 0.3).abs() < 1.5 * FRAME, "landed for {:.3} s", h.time - start);

    // Landing on the run: the running landing for 0.35 s, at the speed that keeps its feet
    // planted (its root doesn't move, so the run's top speed is its own, within 0.7-1.3x).
    h.frames(30, stick(AHEAD));
    h.frame(Input { jump: true, ..stick(AHEAD) });
    while h.sim.state() != MoveState::Ground {
        h.frame(stick(AHEAD));
    }
    assert_eq!(h.clip(), "land_run");
    let before = h.layer("land_run").unwrap().time;
    h.frame(stick(AHEAD));
    let rate = (h.flat_speed() / Tuning::default().ground_speeds.run_max).clamp(0.7, 1.3);
    assert!((h.layer("land_run").unwrap().time - before - rate * FRAME).abs() < 1e-5, "rate {rate}");
    let start = h.time - FRAME;
    while h.clip() == "land_run" {
        h.frame(stick(AHEAD));
    }
    assert_eq!(h.clip(), "run");
    assert!((h.time - start - 0.35).abs() < 1.5 * FRAME, "landed for {:.3} s", h.time - start);
}

#[test]
fn a_respawn_lands_without_a_landing() {
    for respawned in [false, true] {
        let mut h = Host::new();
        h.frames(5, still());
        h.frame(jump());
        h.frames(10, still());
        assert_eq!(h.sim.state(), MoveState::Jump);
        // Put back on the floor with a new sim, as the sandbox respawns him.
        h.sim = Ratchet::new(Tuning::default());
        h.pos = Vec3::ZERO;
        h.touch();
        h.weapons.respawned = respawned;
        h.frame(still());
        assert_eq!(h.clip(), if respawned { "idle" } else { "land" });
    }
}

/// The clip of a jump in aim mode, moving ahead or standing.
fn aimed_jump(slot: Option<Slot>, wrench_out: bool, moving: bool) -> &'static str {
    let mut h = Host::new();
    h.weapons = AnimInput { slot, aiming: true, wrench_throwing: wrench_out, ..Default::default() };
    let dir = if moving { AHEAD } else { Vec2::ZERO };
    h.frames(30, aim(dir, false));
    h.frame(aim(dir, true));
    assert_eq!(h.sim.state(), MoveState::Jump);
    h.clip()
}

#[test]
fn an_aimed_jump_takes_the_strafe_jump_for_its_grip_and_way() {
    assert_eq!(aimed_jump(Some(Slot::BurstPistol), false, true), "strafe_jump_n");
    assert_eq!(aimed_jump(Some(Slot::BurstPistol), false, false), "strafe_jump_idle");
    // The glove (Shatterbomb) has the wrench's grip.
    assert_eq!(aimed_jump(Some(Slot::Shatterbomb), false, true), "wrench_strafe_jump_n");
    // The wrench out of his hand is the melee grip, whatever gun is out.
    assert_eq!(aimed_jump(Some(Slot::BurstPistol), true, true), "wrench_strafe_jump_n");
    // No weapons: nothing to aim, the plain jump.
    assert_eq!(aimed_jump(None, false, true), "jump");
}

#[test]
fn firing_on_the_run_strafes_and_lands_by_direction() {
    // Firing from the hip (the normal speeds, so he runs): the strafe set, then its jump and
    // landing for the way he goes.
    let mut h = Host::new();
    h.weapons = AnimInput { slot: Some(Slot::BurstPistol), trigger: true, ..Default::default() };
    let hip = |jump| Input { move_dir: AHEAD, jump, aim: Some(AHEAD), aim_mode: false, ..Default::default() };
    h.frames(30, hip(false));
    assert_eq!(h.clip(), "strafe_n");
    // No root motion in it: authored at the run's top speed, so 1x at that speed.
    let before = h.layer("strafe_n").unwrap().time;
    h.frame(hip(false));
    let rate = (h.flat_speed() / Tuning::default().ground_speeds.run_max).clamp(0.5, 1.5);
    assert!((h.layer("strafe_n").unwrap().time - before - rate * FRAME).abs() < 1e-4);
    h.frame(hip(true));
    assert_eq!(h.clip(), "strafe_jump_n");
    while h.sim.state() != MoveState::Ground {
        h.frame(hip(false));
    }
    assert_eq!(h.clip(), "strafe_land_n");
    // Strafe changes cut in quickly: over 0.06 s.
    assert!((h.weight("strafe_land_n") - FRAME / 0.06).abs() < 1e-5, "{:?}", h.anim.layers());
}

#[test]
fn glides_with_the_intro_for_his_grip_then_its_loop_leaning_into_turns() {
    let mut h = Host::falling();
    h.weapons.slot = Some(Slot::BurstPistol);
    let hold = |dir: Vec2| Input { jump: true, move_dir: dir, ..Default::default() };
    while h.sim.state() != MoveState::Glide {
        h.frame(hold(Vec2::ZERO));
        assert!(h.time < 2.0, "never glided");
    }
    assert_eq!(h.clip(), "glide_intro_pistol");
    // The lean comes in over 0.25 s, upright (its middle pose).
    let lean = h.layer("glide_lean_additive").unwrap();
    assert_eq!(lean.kind, LayerKind::Additive);
    assert!((lean.weight - FRAME / 0.25).abs() < 1e-5 && (lean.time - 0.25).abs() < 1e-5, "{lean:?}");
    // The intro plays out (2 s), then the loop.
    let start = h.time - FRAME;
    while h.clip() == "glide_intro_pistol" {
        h.frame(hold(Vec2::ZERO));
    }
    assert_eq!(h.clip(), "glide_pistol");
    assert!((h.time - start - 2.0).abs() < 2.5 * FRAME, "intro {:.3} s", h.time - start);
    assert_eq!(h.weight("glide_lean_additive"), 1.0);
    // Steering to his right turns him right: the lean poses past the middle.
    h.frames(15, hold(RIGHT));
    let lean = h.layer("glide_lean_additive").unwrap();
    assert!(lean.time > 0.25 + 0.01, "{lean:?}");
    // Each grip has its own.
    let mut h = Host::falling();
    h.weapons.slot = Some(Slot::Shatterbomb);
    while h.sim.state() != MoveState::Glide {
        h.frame(hold(Vec2::ZERO));
    }
    assert_eq!(h.clip(), "glide_intro_glove");
}

#[test]
fn a_dash_plays_the_clip_for_its_direction_all_through() {
    // Aiming ahead, a dash to his right: the dash to the right (he keeps facing the aim).
    let mut h = Host::new();
    h.weapons = AnimInput { slot: Some(Slot::BurstPistol), aiming: true, ..Default::default() };
    h.frames(10, aim(Vec2::ZERO, false));
    h.frame(Input { dash: true, ..aim(RIGHT, false) });
    assert_eq!((h.sim.state(), h.clip()), (MoveState::Dash, "dash_e"));
    loop {
        h.frame(aim(RIGHT, false));
        if h.sim.state() != MoveState::Dash {
            break;
        }
        assert_eq!(h.clip(), "dash_e");
    }
    // Not aiming, a dash turns him to face where it goes: the one ahead.
    let mut h = Host::new();
    h.frame(Input { dash: true, ..stick(RIGHT) });
    assert_eq!((h.sim.state(), h.clip()), (MoveState::Dash, "dash"));
}

#[test]
fn the_glove_hand_weighs_twenty_times_its_fade() {
    let mut h = Host::new();
    h.weapons.slot = Some(Slot::Shatterbomb);
    for i in 1..=12 {
        h.frame(still());
        let hand = h.layer("glove_hand").unwrap();
        assert_eq!(hand.kind, LayerKind::Base);
        let g = (i as f32 * FRAME / 0.15).min(1.0);
        assert!((hand.weight - 20.0 * g).abs() < 1e-3, "frame {i}: {}", hand.weight);
    }
    // With the glove on, the moves that have wrench-carrying versions use them.
    assert_eq!(h.clip(), "wrench_idle");
    // Back to the pistol: the hand fades out and stops.
    h.weapons.slot = Some(Slot::BurstPistol);
    h.frames(10, still());
    assert!(h.layer("glove_hand").is_none());
    assert_eq!(h.clip(), "pistol_idle");
}

#[test]
fn the_weapon_layers_follow_their_envelopes() {
    let mut h = Host::new();
    h.weapons.slot = Some(Slot::BurstPistol);
    h.frames(5, still());
    assert_eq!(h.clip(), "pistol_idle");
    // A shot: the recoil, in over 0.03 s and out over the last 0.12 s of its 0.4 s.
    h.weapons.shots += 1;
    h.frame(still());
    let fire = h.layer("fire_pistol_additive").unwrap();
    assert_eq!((fire.kind, fire.time), (LayerKind::Additive, FRAME));
    assert!((fire.weight - FRAME / 0.03).abs() < 1e-5);
    h.frames(10, still());
    assert_eq!(h.weight("fire_pistol_additive"), 1.0);
    h.frames(10, still());
    assert!((h.weight("fire_pistol_additive") - (0.4 - 21.0 * FRAME) / 0.12).abs() < 1e-4);
    h.frames(4, still());
    assert!(h.layer("fire_pistol_additive").is_none());

    // A wrench throw, then the catch: the upper body reaches for it, at six times the move
    // clip's weight, and the throw goes into its last 0.25 s.
    h.weapons.wrench_throws += 1;
    h.frame(still());
    assert!((h.weight("throw_additive") - FRAME / 0.08).abs() < 1e-5);
    h.frames(30, still());
    assert_eq!(h.weight("throw_additive"), 1.0);
    h.weapons.wrench_catches += 1;
    h.frame(still());
    let catch = h.layer("throw_catch").unwrap();
    assert_eq!(catch.kind, LayerKind::BaseUpperBody);
    assert!((catch.weight - 6.0 * FRAME / 0.05).abs() < 1e-4, "{catch:?}");
    assert!((h.weight("throw_additive") - (0.25 - FRAME) / 0.25).abs() < 1e-4);

    // Drawing a weapon standing: the whole-body draw for its grip.
    h.weapons.equips += 1;
    h.frame(still());
    assert!((h.weight("draw_pistol_additive") - FRAME / 0.15).abs() < 1e-5);

    // Aiming: the standing pistol aim offsets, scrubbed by where the crosshair is as seen from
    // 0.85 m up: 45° to his right, a quarter of the way along the side poses; 10 m up at 14.1 m
    // out (35.3°), that much of 90° up from the middle of the up/down poses.
    h.weapons.aiming = true;
    h.weapons.aim_target = h.pos + Vec3::new(-10.0, 0.85 + 10.0, 10.0);
    h.frame(still());
    assert_eq!(h.clip(), "aim_idle");
    let (v, side) = (h.layer("aim_pistol_v_additive").unwrap(), h.layer("aim_pistol_h_additive").unwrap());
    assert!((v.weight - FRAME / 0.08).abs() < 1e-5 && side.weight == v.weight);
    let pitch = 10f32.atan2(200f32.sqrt()).to_degrees();
    assert!((v.time - 0.4 * (1.0 - pitch / 90.0)).abs() < 1e-4 && (side.time - 0.2).abs() < 1e-4, "{v:?} {side:?}");
    h.weapons.aiming = false;

    // The Shatterbomb: a throw standing still, the whole-body one; on the move, the upper body's.
    h.weapons.slot = Some(Slot::Shatterbomb);
    h.weapons.grenade_throws += 1;
    h.frame(still());
    assert!(h.layer("grenade_throw_additive").is_some() && h.layer("grenade_throw_arm_additive").is_none());
    h.frames(30, stick(AHEAD));
    h.weapons.grenade_throws += 1;
    h.frame(stick(AHEAD));
    assert!(h.layer("grenade_throw_arm_additive").is_some() && h.layer("grenade_throw_additive").is_none());
    // Winding one up (the button held): the windup, in over 0.08 s from nothing.
    h.frames(60, still());
    h.weapons.cooking = true;
    h.frame(still());
    assert!(h.layer("grenade_windup_additive").is_none(), "it starts at no weight");
    h.frame(still());
    assert!((h.weight("grenade_windup_additive") - FRAME / 0.08).abs() < 1e-5);
}

#[test]
fn sticks_to_a_wall_slides_once_its_clip_is_over_and_jumps_off_slowly() {
    let mut h = Host::on(Level { floor: true, wall_x: Some(1.0), targets: vec![] }, Vec3::new(0.0, 20.0, 0.0));
    while h.sim.state() != MoveState::WallStick {
        h.frame(stick(Vec2::X));
        assert!(h.time < 2.0, "never stuck");
    }
    let stuck = h.clip();
    assert!(stuck == "wall_stick_left" || stuck == "wall_stick_right", "{stuck}");
    // Its clip (1 s here) plays out, then the slide loop.
    let start = h.time - FRAME;
    while h.clip() == stuck {
        h.frame(still());
    }
    assert_eq!(h.clip(), stuck.replace("stick", "slide"));
    assert!((h.time - start - 1.0).abs() < 2.5 * FRAME, "stuck {:.3} s", h.time - start);
    // The wall jump's clip plays at a quarter speed.
    h.frame(jump());
    assert_eq!(h.sim.state(), MoveState::WallJump);
    let clip = h.clip();
    assert!(clip.starts_with("wall_jump_"), "{clip}");
    let first = h.layer(clip).unwrap().time;
    assert!((first - 0.25 * FRAME).abs() < 1e-6);
    h.frame(still());
    assert!((h.layer(clip).unwrap().time - first - 0.25 * FRAME).abs() < 1e-6);
}

#[test]
fn swings_from_the_pull_in_into_the_loop_and_jumps_off() {
    let top = Vec3::new(0.0, 7.0, 4.0);
    let target = SwingTarget { position: top, kind: SwingKind::Swing, axis: AHEAD, length: 5.0, min_range: None, max_range: None, id: 1 };
    let mut h = Host::on(Level { floor: true, wall_x: None, targets: vec![target] }, Vec3::ZERO);
    let swing = |jump, dir| Input { swingshot: true, jump, move_dir: dir, ..Default::default() };
    h.frame(still());
    h.frame(swing(false, Vec2::ZERO));
    assert_eq!(h.sim.state(), MoveState::Swing);
    // Fired from the floor (under 0.9 m up): the pull in, over 0.25 s.
    assert_eq!(h.clip(), "swing_pull");
    assert!((h.weight("swing_pull") - FRAME / 0.25).abs() < 1e-5);
    // Pumping ahead: from the first swing back, the loop.
    while h.clip() != "swing_loop" {
        h.frame(swing(false, AHEAD));
        assert!(h.time < 6.0, "never looped");
    }
    assert_eq!(h.anim.clip(), None, "the loop is a blend, not one clip");
    // Its clips start from nothing, then share its weight (in over 0.35 s) a frame behind, two
    // at most, by the swing's Lean.
    let loop_weight = |h: &Host| SWING_LOOP.iter().map(|n| h.weight(n)).sum::<f32>();
    assert_eq!(loop_weight(&h), 0.0);
    h.frame(swing(false, AHEAD));
    assert!((loop_weight(&h) - FRAME / 0.35).abs() < 1e-4, "{}", loop_weight(&h));
    h.frames(30, swing(false, AHEAD));
    assert!((loop_weight(&h) - 1.0).abs() < 1e-4, "{}", loop_weight(&h));
    let playing = SWING_LOOP.iter().filter(|n| h.weight(n) > 0.0).count();
    assert!((1..=2).contains(&playing), "{:?}", h.anim.layers());
    // Off with a jump: the swing jump, played so its JumpPeak (0.5 s in) comes at the jump's top.
    h.frame(swing(true, Vec2::ZERO));
    assert_eq!((h.sim.state(), h.clip()), (MoveState::SwingJump, "swing_jump"));
    let jump = &h.sim.tuning().swingshot.jump;
    let rate = 0.5 / (2.0 * jump.height_min / jump.gravity()).sqrt();
    assert!((h.layer("swing_jump").unwrap().time - rate * FRAME).abs() < 1e-5, "rate {rate}");
    // The loop's clips fade out with it.
    h.frames(10, still());
    assert_eq!(loop_weight(&h), 0.0);
}

#[test]
fn a_reset_starts_over_and_takes_the_counts_in() {
    let mut h = Host::new();
    h.weapons = AnimInput { slot: Some(Slot::BurstPistol), shots: 7, equips: 3, ..Default::default() };
    h.frame(still());
    // The first update only takes the counts in: no recoil or draw for what came before.
    assert!(h.layer("fire_pistol_additive").is_none() && h.layer("draw_pistol_additive").is_none());
    h.weapons.shots += 1;
    h.frame(still());
    assert!(h.layer("fire_pistol_additive").is_some());
    h.anim.reset();
    assert_eq!(h.anim.clip_name(), None);
    assert!(h.anim.layers().is_empty() && h.anim.sound_clock().is_none());
    h.weapons.shots += 5;
    h.frame(still());
    assert!(h.layer("fire_pistol_additive").is_none());
    assert_eq!(h.anim.layers(), &[Layer { clip: h.index("pistol_idle"), time: FRAME, weight: 1.0, kind: LayerKind::Base }]);
}

#[test]
fn missing_clips_fall_back_as_in_the_sandbox() {
    // Without the walks and the running landing: the run; without a move's clip: the idle.
    let clips: Vec<ClipInfo> = clips().into_iter().filter(|c| !matches!(c.name.as_str(), "walk" | "land_run" | "jump")).collect();
    let mut h = Host::new();
    h.names = clips.iter().map(|c| c.name.clone()).collect();
    h.anim = Animator::new(&clips);
    h.frame(still());
    h.frames(3, stick(AHEAD));
    assert_eq!(h.clip(), "run");
    h.frame(jump());
    assert_eq!((h.sim.state(), h.clip()), (MoveState::Jump, "idle"));
    // No clips at all: nothing plays.
    let mut anim = Animator::new(&[]);
    assert!(anim.update(&h.sim, &AnimInput { dt: FRAME, ..Default::default() }).is_empty());
    assert_eq!(anim.clip_name(), None);
}
