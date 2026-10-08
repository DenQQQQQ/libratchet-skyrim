//! Ledges: the catch on a lip he falls just short of, the placed ledges he grabs on the way
//! down, and the moves off them (see MODLOG.md).

use libratchet::glam::{Vec2, Vec3};
use libratchet::{Contact, Event, Input, Ledge, LedgeFlags, MoveState, RayHit, Ratchet, Tuning, World};

const DT: f32 = 1.0 / 120.0;

/// Boxes and placed ledges, with a floor at y = 0.
struct Boxes {
    boxes: Vec<(Vec3, Vec3)>,
    ledges: Vec<Ledge>,
}

impl Boxes {
    fn inside(&self, p: Vec3) -> bool {
        self.boxes.iter().any(|(lo, hi)| p.cmpgt(*lo + 1e-4).all() && p.cmplt(*hi - 1e-4).all())
    }
}

impl World for Boxes {
    fn ledges(&self, _center: Vec3, _radius: f32, out: &mut Vec<Ledge>) {
        out.extend(self.ledges.iter().copied());
    }

    fn raycast(&self, from: Vec3, to: Vec3) -> Option<RayHit> {
        let d = to - from;
        let mut best: Option<(f32, RayHit)> = None;
        'boxes: for (lo, hi) in &self.boxes {
            let (mut near, mut far) = (0.0f32, best.map_or(1.0, |b| b.0));
            let mut normal = Vec3::ZERO;
            for axis in 0..3 {
                if d[axis].abs() < 1e-9 {
                    if from[axis] < lo[axis] || from[axis] > hi[axis] {
                        continue 'boxes;
                    }
                    continue;
                }
                let inv = 1.0 / d[axis];
                let (enter, exit, n) = if inv > 0.0 {
                    ((lo[axis] - from[axis]) * inv, (hi[axis] - from[axis]) * inv, -Vec3::AXES[axis])
                } else {
                    ((hi[axis] - from[axis]) * inv, (lo[axis] - from[axis]) * inv, Vec3::AXES[axis])
                };
                if enter > near {
                    near = enter;
                    normal = n;
                }
                far = far.min(exit);
                if near > far {
                    continue 'boxes;
                }
            }
            if normal != Vec3::ZERO {
                best = Some((near, RayHit { point: from + d * near, normal }));
            }
        }
        best.map(|b| b.1)
    }
}

/// A host for ledge tests: the character is a point moving through the boxes.
struct Climber {
    r: Ratchet,
    world: Boxes,
    pos: Vec3,
    contact: Contact,
    time: f32,
    events: Vec<(f32, Event)>,
    /// The tick length.
    dt: f32,
}

impl Climber {
    /// A platform 4 m tall whose face looks down -X at x = 1, and him in the air in front of it
    /// facing it, his feet `height` up, `away` out from its face.
    fn before_platform(top: f32, height: f32, away: f32) -> Climber {
        let world = Boxes {
            boxes: vec![
                (Vec3::new(-50.0, -1.0, -50.0), Vec3::new(50.0, 0.0, 50.0)),
                (Vec3::new(1.0, 0.0, -3.0), Vec3::new(5.0, top, 3.0)),
            ],
            ledges: vec![],
        };
        let mut r = Ratchet::new(Tuning::default());
        r.set_facing_yaw(std::f32::consts::FRAC_PI_2);
        let pos = Vec3::new(1.0 - away, height, 0.0);
        let contact = Contact { grounded: false, ground_distance: Some(height), hit_ceiling: false, wall: None, wall_jump_zone: false, landing_ahead: false };
        let mut c = Climber { r, world, pos, contact, time: 0.0, events: vec![], dt: DT };
        // Into the air: the next tick he leaves the ground, as if he'd walked off an edge.
        c.r.step_in(DT, &Input::default(), &Contact { grounded: true, ..c.contact }, c.pos, &c.world);
        c
    }

    /// A placed ledge along the platform's face.
    fn with_ledge(mut self, id: u32, y: f32) -> Climber {
        self.world.ledges.push(Ledge {
            a: Vec3::new(1.0, y, -3.0),
            b: Vec3::new(1.0, y, 3.0),
            normal: Vec2::new(-1.0, 0.0),
            id,
            flags: LedgeFlags::default(),
        });
        self
    }

    fn tick_with(&mut self, input: Input) {
        let dt = self.dt;
        let v = self.r.step_in(dt, &input, &self.contact, self.pos, &self.world);
        self.events.extend(self.r.events().iter().map(|e| (self.time, *e)));
        self.time += dt;
        let mut p = self.pos;
        // Down onto tops, then sideways unless that goes into a box.
        let mut grounded = false;
        let next_y = p.y + v.y * dt;
        if v.y <= 0.0 {
            let mut floor = f32::NEG_INFINITY;
            for (lo, hi) in &self.world.boxes {
                let over = p.x > lo.x && p.x < hi.x && p.z > lo.z && p.z < hi.z;
                if over && p.y >= hi.y - 1e-3 && next_y <= hi.y {
                    floor = floor.max(hi.y);
                }
            }
            if floor > f32::NEG_INFINITY {
                p.y = floor;
                grounded = true;
            } else {
                p.y = next_y;
            }
        } else if !self.world.inside(Vec3::new(p.x, next_y, p.z)) {
            p.y = next_y;
        }
        for axis in [0, 2] {
            let mut n = p;
            n[axis] += v[axis] * dt;
            if !self.world.inside(n) {
                p = n;
            }
        }
        self.pos = p;
        let below = self.world.raycast(p + Vec3::Y * 0.01, p - Vec3::Y * 100.0).map(|h| p.y - h.point.y);
        grounded |= below.is_some_and(|d| d.abs() < 1e-3);
        self.contact = Contact { grounded, ground_distance: below, hit_ceiling: false, wall: None, wall_jump_zone: false, landing_ahead: false };
    }

    fn tick(&mut self, jump: bool, crouch: bool, stick: Vec2) {
        self.tick_with(Input { move_dir: stick, jump, crouch, ..Default::default() });
    }

    /// Runs until `done` or `secs` pass.
    fn until(&mut self, secs: f32, done: impl Fn(&Climber) -> bool) {
        let start = self.time;
        while !done(self) && self.time - start < secs {
            self.tick(false, false, Vec2::ZERO);
        }
    }

    fn first(&self, pred: impl Fn(&Event) -> bool) -> Option<f32> {
        self.events.iter().find(|(_, e)| pred(e)).map(|(t, _)| *t)
    }
}

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn catches_a_lip_it_falls_just_short_of_and_climbs_up_with_a_jump() {
    let t = Tuning::default();
    // Falling straight down a metre out from a 4 m platform, starting above its top.
    let mut c = Climber::before_platform(4.0, 4.6, 1.0);
    c.until(2.0, |c| c.r.state() == MoveState::LedgeHang);
    assert_eq!(c.r.state(), MoveState::LedgeHang, "never caught it, ended at {:?}", c.pos);
    assert!(c.first(|e| *e == Event::LedgeGrabbed { caught: true }).is_some());
    // Caught with the lip a little above his feet, as the catch clip has it.
    let (lip, normal) = c.r.ledge().unwrap();
    assert!(close(lip.y, 4.0, 0.01) && close(lip.x, 1.0, 0.01) && normal == Vec2::new(-1.0, 0.0), "lip {lip:?} {normal:?}");
    // Into the hang: 0.5 m out, 1.29 m below the lip.
    c.until(1.0, |c| c.r.ledge_move().is_none());
    for _ in 0..10 {
        c.tick(false, false, Vec2::ZERO);
    }
    let hang = Vec3::new(1.0 - t.ledge.hang_out, 4.0 - t.ledge.hang_down, 0.0);
    assert!(c.pos.distance(hang) < 0.02, "hangs at {:?}, not {hang:?}", c.pos);
    // Hanging on until a button.
    for _ in 0..240 {
        c.tick(false, false, Vec2::ZERO);
    }
    assert_eq!(c.r.state(), MoveState::LedgeHang);
    // Jump: up and onto the top.
    c.tick(true, false, Vec2::ZERO);
    assert_eq!(c.r.state(), MoveState::LedgeClimb);
    c.until(2.0, |c| c.r.state() == MoveState::Ground);
    assert_eq!(c.r.state(), MoveState::Ground, "ended at {:?}", c.pos);
    assert!(close(c.pos.y, 4.0, 0.01) && c.pos.x > 1.1, "stood at {:?}", c.pos);
}

#[test]
fn no_catch_without_room_to_hang_below() {
    // A 2.5 m platform: under 3 m of drop below the hang.
    let mut c = Climber::before_platform(2.5, 3.1, 1.0);
    c.until(2.0, |c| c.r.state() == MoveState::Ground);
    assert_eq!(c.r.state(), MoveState::Ground);
    assert!(c.first(|e| matches!(e, Event::LedgeGrabbed { .. })).is_none());
}

#[test]
fn the_drop_button_lets_go_of_a_caught_ledge() {
    let mut c = Climber::before_platform(4.0, 4.6, 1.0);
    c.until(2.0, |c| c.r.state() == MoveState::LedgeHang && c.r.ledge_move().is_none());
    c.tick(false, true, Vec2::ZERO);
    assert_eq!(c.r.state(), MoveState::Fall);
    assert!(c.first(|e| *e == Event::LedgeDropped).is_some());
    c.until(2.0, |c| c.r.state() == MoveState::Ground);
    assert!(close(c.pos.y, 0.0, 0.01), "landed at {:?}", c.pos);
}

#[test]
fn grabs_a_placed_ledge_on_the_way_down_and_jumps_up_over_it() {
    let t = Tuning::default();
    // A placed ledge on a 4 m platform, him coming down 0.7 m out with the lip 1.5 m over his
    // feet: the FromAir window takes it straight away.
    let mut c = Climber::before_platform(4.0, 2.5, 0.7).with_ledge(1, 4.0);
    c.tick(false, false, Vec2::ZERO);
    assert_eq!(c.r.state(), MoveState::LedgeHang, "didn't grab, at {:?}", c.pos);
    assert!(c.first(|e| *e == Event::LedgeGrabbed { caught: false }).is_some());
    c.until(1.0, |c| c.r.ledge_move().is_none());
    c.tick(false, false, Vec2::ZERO);
    let hang = Vec3::new(1.0 - t.ledge.hang_out, 4.0 - t.ledge.hang_down, 0.0);
    assert!(c.pos.distance(hang) < 0.02, "hangs at {:?}", c.pos);
    // Jump with no other ledge about: the ledge jump, at 2 m/s straight at the wall.
    c.tick(true, false, Vec2::ZERO);
    assert_eq!(c.r.state(), MoveState::LedgeJump);
    assert!(c.first(|e| *e == Event::LedgeJumped).is_some());
    // From its first frame on.
    c.tick(false, false, Vec2::ZERO);
    let speed = Vec2::new(c.r.velocity().x, c.r.velocity().z);
    assert!(close(speed.x, t.ledge_jump_speeds.run_max, 0.01) && speed.y.abs() < 1e-4, "speed {speed:?}");
    c.until(2.0, |c| c.r.state() == MoveState::Ground);
    assert!(close(c.pos.y, 4.0, 0.01) && c.pos.x > 1.0, "landed at {:?}", c.pos);
}

#[test]
fn jumps_from_a_placed_ledge_to_one_above_and_drops_with_a_turn() {
    let t = Tuning::default();
    let mut c = Climber::before_platform(8.0, 2.5, 0.7).with_ledge(1, 4.0).with_ledge(2, 6.5);
    c.until(1.0, |c| c.r.state() == MoveState::LedgeHang && c.r.ledge_move().is_none());
    assert!(close(c.r.ledge().unwrap().0.y, 4.0, 0.01));
    // Jump: to the ledge above, by the clip that goes up to a ledge.
    c.tick(true, false, Vec2::ZERO);
    assert!(c.first(|e| *e == Event::LedgeTransferred).is_some());
    assert!(close(c.r.ledge().unwrap().0.y, 6.5, 0.01));
    c.until(2.0, |c| c.r.ledge_move().is_none());
    c.tick(false, false, Vec2::ZERO);
    let hang = Vec3::new(1.0 - t.ledge.hang_out, 6.5 - t.ledge.hang_down, 0.0);
    assert!(c.pos.distance(hang) < 0.02, "hangs at {:?}", c.pos);
    // The drop button: let go with the turn, then fall, facing away from the wall.
    c.tick(false, true, Vec2::ZERO);
    assert_eq!(c.r.state(), MoveState::LedgeDrop);
    c.until(1.0, |c| c.r.state() != MoveState::LedgeDrop);
    assert_eq!(c.r.state(), MoveState::Fall);
    let facing = Vec2::new(c.r.facing_yaw().sin(), c.r.facing_yaw().cos());
    assert!(facing.dot(Vec2::new(-1.0, 0.0)) > 0.99, "faces {facing:?}");
    // Not back onto the ledge it let go of on the way down.
    c.until(3.0, |c| c.r.state() == MoveState::Ground || c.r.state() == MoveState::LedgeHang);
    assert!(c.r.ledge().is_none_or(|(p, _)| !close(p.y, 6.5, 0.01)));
}

/// Him standing on a box `stand` tall (x from -5 to 0, facing +X) and a second box `top` tall
/// past a 1 m gap (x from 1).
fn on_a_box(stand: f32, top: f32, ledges: Vec<Ledge>) -> Climber {
    let world = Boxes {
        boxes: vec![
            (Vec3::new(-50.0, -1.0, -50.0), Vec3::new(50.0, 0.0, 50.0)),
            (Vec3::new(-5.0, 0.0, -3.0), Vec3::new(0.0, stand, 3.0)),
            (Vec3::new(1.0, 0.0, -3.0), Vec3::new(5.0, top, 3.0)),
        ],
        ledges,
    };
    let mut r = Ratchet::new(Tuning::default());
    r.set_facing_yaw(std::f32::consts::FRAC_PI_2);
    Climber { r, world, pos: Vec3::new(-0.3, stand, 0.0), contact: Contact::default(), time: 0.0, events: vec![], dt: DT }
}

#[test]
fn catches_a_lip_ahead_coming_down_from_a_jump() {
    // A jump from a 5 m box at a 6 m one that falls short: caught on the way down (the jump's
    // search, ahead only).
    let mut c = on_a_box(5.0, 6.0, vec![]);
    let toward = Vec2::new(1.0, 0.0);
    for _ in 0..24 {
        c.tick(true, false, toward);
    }
    c.until(3.0, |c| matches!(c.r.state(), MoveState::LedgeHang | MoveState::Ground));
    assert_eq!(c.r.state(), MoveState::LedgeHang, "never caught it, ended at {:?}", c.pos);
    assert!(close(c.r.ledge().unwrap().0.y, 6.0, 0.01));
}

#[test]
fn walking_off_a_placed_ledge_does_not_grab_it_behind_him() {
    // A 2 m box with a placed ledge along its edge: walking off it, the ledge is behind him.
    let edge = Ledge { a: Vec3::new(0.0, 2.0, -3.0), b: Vec3::new(0.0, 2.0, 3.0), normal: Vec2::new(1.0, 0.0), id: 1, flags: LedgeFlags::default() };
    let mut c = on_a_box(2.0, 0.5, vec![edge]);
    c.until(2.0, |c| c.pos.x > 0.4 || c.r.state() == MoveState::LedgeHang);
    let away = Vec2::new(1.0, 0.0);
    while c.time < 3.0 && !matches!(c.r.state(), MoveState::LedgeHang) && !(c.r.state() == MoveState::Ground && c.pos.y < 1.0) {
        c.tick(false, false, away);
    }
    assert!(c.first(|e| matches!(e, Event::LedgeGrabbed { .. })).is_none(), "grabbed the ledge behind him at {:?}", c.pos);
}

#[test]
fn a_jump_press_on_a_ledge_without_the_ledge_jump_hangs_on() {
    let mut c = Climber::before_platform(4.0, 2.5, 0.7).with_ledge(1, 4.0);
    c.world.ledges[0].flags = LedgeFlags { jump: false, ..LedgeFlags::default() };
    c.until(1.0, |c| c.r.state() == MoveState::LedgeHang && c.r.ledge_move().is_none());
    c.tick(true, false, Vec2::ZERO);
    for _ in 0..30 {
        c.tick(false, false, Vec2::ZERO);
    }
    assert_eq!(c.r.state(), MoveState::LedgeHang, "let go on a plain jump press");
    // The drop button still lets go.
    c.tick(false, true, Vec2::ZERO);
    assert_eq!(c.r.state(), MoveState::LedgeDrop);
}

#[test]
fn a_placed_ledge_taken_away_drops_him() {
    let mut c = Climber::before_platform(4.0, 2.5, 0.7).with_ledge(1, 4.0);
    c.until(1.0, |c| c.r.state() == MoveState::LedgeHang && c.r.ledge_move().is_none());
    c.world.ledges.clear();
    c.tick(false, false, Vec2::ZERO);
    assert_eq!(c.r.state(), MoveState::Fall);
}

#[test]
fn a_fast_fall_catches_the_lip_at_30_hz() {
    // From 6 to 16 m above the lip, at 30 ticks a second he falls up to 1.7 m a tick, more than
    // the catch window; the catch sweeps the tick's fall.
    let mut missed = vec![];
    for i in 0..40 {
        let height = 10.0 + i as f32 * 0.25;
        let mut c = Climber::before_platform(4.0, height, 1.0);
        c.dt = 1.0 / 30.0;
        c.until(5.0, |c| c.r.state() == MoveState::LedgeHang || (c.r.state() == MoveState::Ground && c.pos.y < 0.5));
        if c.r.state() != MoveState::LedgeHang {
            missed.push(height);
        }
    }
    assert!(missed.is_empty(), "missed the lip falling from {missed:?} m");
}
