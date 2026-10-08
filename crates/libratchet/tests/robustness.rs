//! Whatever a host feeds it, the sim must hand back a finite velocity: one bad tick would throw
//! the character to infinity. This drives it with random play (stick, buttons, camera, aiming),
//! odd tick lengths (zero, long hitches), crude collision, and contacts that sometimes contradict
//! what happened (grounded in mid-air, walls that aren't there, zero normals), in a world with a
//! ledge and Swingshot targets. `ROBUST_SEEDS=5000 cargo test --release --test robustness` plays
//! more games than the default 96.

use libratchet::glam::{Vec2, Vec3};
use libratchet::{Contact, Event, Input, Ledge, LedgeFlags, RayHit, Ratchet, SwingKind, SwingTarget, Tuning, WallContact, World};

/// A small deterministic generator (xorshift64*), so a failing seed replays.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    fn dir2(&mut self) -> Vec2 {
        Vec2::from_angle(self.range(0.0, std::f32::consts::TAU))
    }
}

/// The floor (y = 0) and a 4 m block whose -X face has a placed ledge along its top.
const BLOCK: (Vec3, Vec3) = (Vec3::new(2.0, 0.0, -3.0), Vec3::new(6.0, 4.0, 3.0));

struct Course;

impl World for Course {
    fn ledges(&self, _center: Vec3, _radius: f32, out: &mut Vec<Ledge>) {
        out.push(Ledge {
            a: Vec3::new(BLOCK.0.x, BLOCK.1.y, BLOCK.0.z),
            b: Vec3::new(BLOCK.0.x, BLOCK.1.y, BLOCK.1.z),
            normal: Vec2::new(-1.0, 0.0),
            id: 1,
            flags: LedgeFlags { grab: true, jump: true, drop: true, transfer: true },
        });
        // A host's mistake: a ledge with no normal.
        out.push(Ledge { a: Vec3::new(-3.0, 2.0, -3.0), b: Vec3::new(-3.0, 2.0, 3.0), normal: Vec2::ZERO, id: 2, flags: LedgeFlags::default() });
    }

    fn raycast(&self, from: Vec3, to: Vec3) -> Option<RayHit> {
        let boxes = [(Vec3::new(-100.0, -1.0, -100.0), Vec3::new(100.0, 0.0, 100.0)), BLOCK];
        let d = to - from;
        let mut best: Option<(f32, Vec3)> = None;
        'boxes: for (lo, hi) in boxes {
            let (mut near, mut far, mut normal) = (0.0f32, best.map_or(1.0, |b| b.0), Vec3::ZERO);
            for axis in 0..3 {
                if d[axis].abs() < 1e-9 {
                    if from[axis] < lo[axis] || from[axis] > hi[axis] {
                        continue 'boxes;
                    }
                    continue;
                }
                let inv = 1.0 / d[axis];
                let (enter, exit, face) = if inv > 0.0 {
                    ((lo[axis] - from[axis]) * inv, (hi[axis] - from[axis]) * inv, -Vec3::AXES[axis])
                } else {
                    ((hi[axis] - from[axis]) * inv, (lo[axis] - from[axis]) * inv, Vec3::AXES[axis])
                };
                if enter > near {
                    near = enter;
                    normal = face;
                }
                far = far.min(exit);
                if near > far {
                    continue 'boxes;
                }
            }
            if normal != Vec3::ZERO {
                best = Some((near, normal));
            }
        }
        best.map(|(t, normal)| RayHit { point: from + d * t, normal })
    }

    fn swing_targets(&self, _center: Vec3, _radius: f32, out: &mut Vec<SwingTarget>) {
        let target = |position, kind, length, id| SwingTarget { position, kind, axis: Vec2::new(0.0, 1.0), length, min_range: None, max_range: None, id };
        out.push(target(Vec3::new(-2.0, 8.0, 6.0), SwingKind::Swing, 5.0, 1));
        out.push(target(Vec3::new(-8.0, 6.0, -2.0), SwingKind::Hook, -1.0, 2));
        // One right above the block's ledge, and one he can stand under.
        out.push(target(Vec3::new(1.0, 9.0, 0.0), SwingKind::Swing, 3.0, 3));
        out.push(target(Vec3::new(-4.0, 3.0, -6.0), SwingKind::Swing, 4.0, 4));
        // A host's mistakes: a swing with no cable, and one nowhere.
        out.push(target(Vec3::new(3.0, 7.0, -4.0), SwingKind::Swing, 0.0, 5));
        out.push(target(Vec3::new(f32::NAN, 7.0, 4.0), SwingKind::Swing, 5.0, 6));
    }
}

/// Moves him out of the floor and the block; what he touched.
fn collide(pos: &mut Vec3) -> Contact {
    // (`Contact::default()` stands him on the ground.)
    let mut contact = Contact { grounded: false, ground_distance: None, ..Default::default() };
    if pos.y <= 0.0 {
        pos.y = 0.0;
        contact.grounded = true;
    }
    let (lo, hi) = BLOCK;
    if pos.x > lo.x && pos.x < hi.x && pos.z > lo.z && pos.z < hi.z && pos.y < hi.y {
        let pushes = [(pos.x - lo.x, Vec3::NEG_X), (hi.x - pos.x, Vec3::X), (pos.z - lo.z, Vec3::NEG_Z), (hi.z - pos.z, Vec3::Z), (hi.y - pos.y, Vec3::Y)];
        let (depth, normal) = pushes.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
        *pos += normal * depth;
        if normal == Vec3::Y {
            contact.grounded = true;
        } else {
            contact.wall = Some(WallContact { normal: Vec2::new(normal.x, normal.z), sticky: true, distance: None });
        }
    }
    let on_block = pos.x >= lo.x && pos.x <= hi.x && pos.z >= lo.z && pos.z <= hi.z && pos.y >= hi.y;
    contact.ground_distance = Some(if on_block { pos.y - hi.y } else { pos.y });
    contact
}

fn play(seed: u64, ticks: usize) {
    let mut rng = Rng(seed);
    let scale = if rng.chance(0.3) { 1.55 } else { 1.0 };
    let mut sim = Ratchet::new(Tuning::default().scaled(scale));
    let mut pos = Vec3::new(rng.range(-6.0, 0.0), 0.0, rng.range(-6.0, 6.0));
    let mut contact = Contact { grounded: true, ground_distance: Some(0.0), ..Default::default() };
    let mut input = Input { view_forward: Vec3::Z, view_right: Vec2::new(-1.0, 0.0), ..Default::default() };
    for tick in 0..ticks {
        // A player's inputs: held for a while, then changed.
        if rng.chance(0.08) {
            input.move_dir = if rng.chance(0.2) { Vec2::ZERO } else { rng.dir2() * rng.range(0.0, 1.0) };
        }
        input.jump ^= rng.chance(0.06);
        input.dash ^= rng.chance(0.03);
        input.melee ^= rng.chance(0.03);
        input.crouch ^= rng.chance(0.03);
        input.swingshot ^= rng.chance(0.05);
        if rng.chance(0.02) {
            input.aim = rng.chance(0.4).then(|| rng.dir2());
            input.aim_mode = rng.chance(0.5);
        }
        if rng.chance(0.05) {
            // The camera, sometimes missing (zero), sometimes straight up or down.
            input.view_forward = match rng.unit() {
                u if u < 0.1 => Vec3::ZERO,
                u if u < 0.15 => Vec3::Y * if rng.chance(0.5) { 1.0 } else { -1.0 },
                _ => {
                    let d = rng.dir2();
                    Vec3::new(d.x, rng.range(-0.8, 0.8), d.y).normalize()
                }
            };
            let flat = Vec2::new(input.view_forward.x, input.view_forward.z);
            input.view_right = Vec2::new(-flat.y, flat.x).normalize_or_zero();
        }
        if rng.chance(0.01) {
            sim.set_tuning(Tuning::default().scaled(if rng.chance(0.5) { 1.0 } else { 1.55 }));
        }
        // Tick lengths: mostly a frame, sometimes nothing, sometimes a long hitch.
        let dt = match rng.unit() {
            u if u < 0.02 => 0.0,
            u if u < 0.04 => rng.range(0.05, 0.3),
            _ => rng.range(1.0 / 144.0, 1.0 / 30.0),
        };
        let v = sim.step_in(dt, &input, &contact, pos, &Course);
        let state = sim.state();
        assert!(v.is_finite() && v.length() < 1000.0, "seed {seed:#x} tick {tick}: velocity {v:?} in {state:?} at {pos:?}");
        pos += v * dt;
        contact = collide(&mut pos);
        // Hosts get things wrong too: contacts that don't match what happened.
        if rng.chance(0.02) {
            contact.grounded = !contact.grounded;
        }
        if rng.chance(0.02) {
            let normal = if rng.chance(0.2) { Vec2::ZERO } else { rng.dir2() };
            contact.wall = Some(WallContact { normal, sticky: rng.chance(0.5), distance: rng.chance(0.5).then(|| rng.range(-0.5, 1.5)) });
        }
        if rng.chance(0.02) {
            contact.ground_distance = rng.chance(0.5).then(|| rng.range(-1.0, 20.0));
        }
        contact.hit_ceiling = rng.chance(0.01);
        contact.wall_jump_zone ^= rng.chance(0.01);
        contact.landing_ahead = rng.chance(0.3);
        assert!(pos.is_finite(), "seed {seed:#x} tick {tick}: position {pos:?} in {state:?}");
    }
}

#[test]
fn random_play_never_makes_a_non_finite_velocity() {
    let seeds = std::env::var("ROBUST_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(96u64);
    for seed in 1..=seeds {
        play(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), 4000);
    }
}

#[test]
fn a_tick_of_no_time_changes_nothing_and_keeps_its_press() {
    let mut sim = Ratchet::new(Tuning::default());
    let ground = Contact::default();
    // Running and turning (a zero tick mid-turn used to leave his facing NaN).
    let run = Input { move_dir: Vec2::new(1.0, 0.0), ..Default::default() };
    for _ in 0..10 {
        sim.step(1.0 / 60.0, &run, &ground, Vec3::ZERO);
    }
    let before = sim.velocity();
    let jump = Input { jump: true, ..run };
    for dt in [0.0, -0.01, f32::NAN, f32::INFINITY] {
        assert_eq!(sim.step(dt, &jump, &ground, Vec3::ZERO), before, "a {dt} s tick moved him");
        assert!(sim.events().is_empty());
    }
    sim.step(1.0 / 60.0, &jump, &ground, Vec3::ZERO);
    assert!(sim.events().contains(&Event::Jumped), "the press was lost");
    assert!(sim.facing_yaw().is_finite() && sim.velocity().is_finite());
}
