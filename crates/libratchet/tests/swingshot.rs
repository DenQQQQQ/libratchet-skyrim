//! The Swingshot and the Hookshot (see MODLOG.md).

use libratchet::glam::{Vec2, Vec3};
use libratchet::{Contact, Event, Input, Ledge, MoveState, RayHit, Ratchet, SwingKind, SwingTarget, Tuning, World};

const DT: f32 = 1.0 / 120.0;

/// A floor at y = 0, some targets, and maybe a wall across Z at this z.
struct Targets(Vec<SwingTarget>, Option<f32>);

impl World for Targets {
    fn ledges(&self, _center: Vec3, _radius: f32, _out: &mut Vec<Ledge>) {}

    fn raycast(&self, from: Vec3, to: Vec3) -> Option<RayHit> {
        let mut hits = vec![];
        if (from.y > 0.0) != (to.y > 0.0) {
            hits.push((from.y / (from.y - to.y), Vec3::Y));
        }
        if let Some(z) = self.1
            && (from.z > z) != (to.z > z)
        {
            hits.push(((from.z - z) / (from.z - to.z), Vec3::Z * (from.z - z).signum()));
        }
        hits.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(t, normal)| RayHit { point: from + (to - from) * t, normal })
    }

    fn swing_targets(&self, _center: Vec3, _radius: f32, out: &mut Vec<SwingTarget>) {
        out.extend(self.0.iter().copied());
    }
}

fn target(position: Vec3, kind: SwingKind, length: f32) -> SwingTarget {
    SwingTarget { position, kind, axis: Vec2::new(0.0, 1.0), length, min_range: None, max_range: None, id: 1 }
}

struct Swinger {
    r: Ratchet,
    world: Targets,
    pos: Vec3,
    contact: Contact,
    time: f32,
    events: Vec<(f32, Event)>,
    /// The camera: behind him, looking ahead (+Z) and a little up.
    view: Vec3,
}

impl Swinger {
    fn new(targets: Vec<SwingTarget>) -> Swinger {
        Swinger {
            r: Ratchet::new(Tuning::default()),
            world: Targets(targets, None),
            pos: Vec3::ZERO,
            contact: Contact::default(),
            time: 0.0,
            events: vec![],
            view: Vec3::new(0.0, 0.3, 1.0).normalize(),
        }
    }

    fn tick(&mut self, swingshot: bool, jump: bool, stick: Vec2) {
        let view_right = Vec2::new(-self.view.z, self.view.x).normalize_or_zero();
        let input = Input { move_dir: stick, jump, swingshot, view_forward: self.view.normalize(), view_right, ..Default::default() };
        let v = self.r.step_in(DT, &input, &self.contact, self.pos, &self.world);
        self.events.extend(self.r.events().iter().map(|e| (self.time, *e)));
        self.time += DT;
        self.pos += v * DT;
        let grounded = self.pos.y <= 0.0;
        if grounded {
            self.pos.y = 0.0;
        }
        self.contact = Contact { grounded, ground_distance: Some(self.pos.y.max(0.0)), ..Contact::default() };
    }

    fn saw(&self, pred: impl Fn(&Event) -> bool) -> bool {
        self.events.iter().any(|(_, e)| pred(e))
    }
}

#[test]
fn swings_on_a_target_never_above_it_and_jumps_off() {
    let t = Tuning::default();
    let top = Vec3::new(0.0, 7.0, 4.0);
    let mut s = Swinger::new(vec![target(top, SwingKind::Swing, 5.0)]);
    s.tick(false, false, Vec2::ZERO);
    assert_eq!(s.r.swing_target(), Some(top), "the target in view wasn't picked");
    // Hold the button: he's on it.
    s.tick(true, false, Vec2::ZERO);
    assert_eq!(s.r.state(), MoveState::Swing);
    assert!(s.saw(|e| *e == Event::SwingAttached { hook: false }));
    // Swinging, pumping ahead: the cable reels to its length, he goes past the target and back,
    // and never above it.
    let mut highest = f32::MIN;
    let mut phases = vec![];
    for _ in 0..480 {
        s.tick(true, false, Vec2::new(0.0, 1.0));
        highest = highest.max(s.pos.y);
        if let Some(p) = s.r.swing_phase()
            && phases.last() != Some(&p)
        {
            phases.push(p);
        }
    }
    assert_eq!(s.r.state(), MoveState::Swing);
    assert_eq!(phases, vec![0, 1, 2], "phases {phases:?}");
    assert!(highest <= top.y + 0.05, "went {:.2} m above the target", highest - top.y);
    let (_, length) = s.r.swinging_on().unwrap();
    assert!((length - 5.0).abs() < 0.05, "cable {length:.2} m");
    assert!((s.pos.distance(top) - 5.0).abs() < 0.1, "{:.2} m from the target", s.pos.distance(top));
    // Jump: off, 8 m/s up, at least the swing jump's speed ahead, from its first tick.
    s.tick(true, true, Vec2::ZERO);
    assert_eq!(s.r.state(), MoveState::SwingJump);
    let v = s.r.velocity();
    assert!(Vec2::new(v.x, v.z).length() >= t.swingshot.jump_min_speed - 0.01, "first tick ahead at {:.2} m/s", Vec2::new(v.x, v.z).length());
    s.tick(true, true, Vec2::ZERO);
    let v = s.r.velocity();
    let flat = Vec2::new(v.x, v.z).length();
    assert!(flat >= t.swingshot.jump_min_speed - 0.01, "ahead at {flat:.2} m/s");
    assert!((v.y - 8.0).abs() < 0.3, "up at {:.2} m/s", v.y);
    // At its top, a fall.
    let start = s.time;
    while s.r.state() == MoveState::SwingJump && s.time - start < 2.0 {
        s.tick(false, false, Vec2::ZERO);
    }
    assert_eq!(s.r.state(), MoveState::Fall);
    assert!((s.time - start - t.swingshot.jump.time_to_peak).abs() < 0.05, "fell after {:.2} s", s.time - start);
}

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Hooking on to a target whose line runs across the way he's going, he turns to the swing at
/// the air turn's pace (at most 1500°/s), never in one tick (RiftApart.exe: the swing sets no
/// turn of its own; the mover turns him toward the way he moves).
#[test]
fn hooking_on_turns_him_instead_of_snapping() {
    let top = Vec3::new(6.0, 6.0, 1.5);
    let mut s = Swinger::new(vec![target(top, SwingKind::Swing, 5.0)]);
    s.view = Vec3::new(1.0, 0.3, 0.0).normalize();
    // Running along +X, a jump, and the stick let go as he hooks on.
    for _ in 0..60 {
        s.tick(false, false, Vec2::X);
    }
    s.tick(false, true, Vec2::X);
    for _ in 0..10 {
        s.tick(false, false, Vec2::X);
    }
    assert_eq!(s.r.state(), MoveState::Jump);
    assert_eq!(s.r.swing_target(), Some(top), "the target ahead wasn't picked");
    let mut last = s.r.facing_yaw();
    let start = last;
    let mut biggest = 0.0f32;
    for i in 0..60 {
        s.tick(true, false, Vec2::ZERO);
        assert_eq!(s.r.state(), MoveState::Swing, "tick {i}");
        let yaw = s.r.facing_yaw();
        biggest = biggest.max(wrap(yaw - last).abs());
        last = yaw;
    }
    assert!(biggest <= (1500.0f32 * DT).to_radians() + 1e-3, "turned {:.1}° in one tick", biggest.to_degrees());
    // He came round toward the swing's line (+Z), from facing +X.
    assert!(wrap(last - start).abs() > 30f32.to_radians(), "turned only {:.1}°", wrap(last - start).to_degrees());
}

/// A target off to his side, its line along +X, in a camera looking that way.
fn side_swing() -> Swinger {
    let top = Vec3::new(5.0, 6.0, 1.0);
    let mut s = Swinger::new(vec![SwingTarget { axis: Vec2::new(1.0, 0.0), ..target(top, SwingKind::Swing, 5.0) }]);
    s.view = Vec3::new(1.0, 0.3, 0.3).normalize();
    s
}

/// After a hop in place with the stick let go, the jump's turn law is nil, and the swing keeps
/// it: he keeps his facing all through the swing (RiftApart.exe, as its code reads; worth a
/// look in the game).
#[test]
fn a_swing_after_a_hop_with_the_stick_let_go_keeps_his_facing() {
    let mut s = side_swing();
    s.tick(false, true, Vec2::ZERO);
    for _ in 0..20 {
        s.tick(false, false, Vec2::ZERO);
    }
    let start = s.r.facing_yaw();
    for _ in 0..240 {
        s.tick(true, false, Vec2::ZERO);
    }
    assert_eq!(s.r.state(), MoveState::Swing);
    assert!(wrap(s.r.facing_yaw() - start).abs() < 1e-3, "turned {:.1}°", wrap(s.r.facing_yaw() - start).to_degrees());
}

/// Standing still after that hop, he takes the game's Idle turn law, so a swing fired standing
/// turns him to the way he swings.
#[test]
fn a_swing_fired_standing_turns_him_whatever_came_before() {
    let mut s = side_swing();
    s.tick(false, true, Vec2::ZERO);
    for _ in 0..240 {
        s.tick(false, false, Vec2::ZERO);
    }
    assert_eq!(s.r.state(), MoveState::Ground);
    let start = s.r.facing_yaw();
    for _ in 0..240 {
        s.tick(true, false, Vec2::ZERO);
    }
    assert_eq!(s.r.state(), MoveState::Swing);
    assert!(wrap(s.r.facing_yaw() - start).abs() > 45f32.to_radians(), "turned only {:.1}°", wrap(s.r.facing_yaw() - start).to_degrees());
}

/// The game's swing-loop animation values: Lean heads for 0 swinging ahead and 1 swinging back,
/// and both values stay between 0 and 1.
#[test]
fn the_swing_leans_ahead_then_back() {
    let top = Vec3::new(0.0, 7.0, 4.0);
    let mut s = Swinger::new(vec![target(top, SwingKind::Swing, 5.0)]);
    s.tick(false, false, Vec2::ZERO);
    s.tick(true, false, Vec2::ZERO);
    let (mut ahead, mut back) = (1.0f32, 0.0f32);
    for _ in 0..480 {
        s.tick(true, false, Vec2::new(0.0, 1.0));
        let (lean, speed) = s.r.swing_lean_speed().expect("not swinging");
        assert!((0.0..=1.0).contains(&lean) && (0.0..=1.0).contains(&speed), "lean {lean}, speed {speed}");
        let v = s.r.velocity();
        if v.z > 3.0 {
            ahead = ahead.min(lean);
        } else if v.z < -3.0 {
            back = back.max(lean);
        }
    }
    assert!(ahead < 0.2 && back > 0.8, "lean swinging ahead {ahead:.2}, back {back:.2}");
}

#[test]
fn targets_out_of_view_out_of_range_or_far_below_arent_picked() {
    // Behind him.
    let mut s = Swinger::new(vec![target(Vec3::new(0.0, 6.0, -4.0), SwingKind::Swing, 5.0)]);
    s.tick(false, false, Vec2::ZERO);
    assert_eq!(s.r.swing_target(), None);
    // Too far.
    let mut s = Swinger::new(vec![target(Vec3::new(0.0, 6.0, 30.0), SwingKind::Swing, 5.0)]);
    s.tick(false, false, Vec2::ZERO);
    assert_eq!(s.r.swing_target(), None);
    // In view and range.
    let mut s = Swinger::new(vec![target(Vec3::new(0.0, 6.0, 10.0), SwingKind::Swing, 5.0)]);
    s.tick(false, false, Vec2::ZERO);
    assert!(s.r.swing_target().is_some());
    // From 15 m up, looking down: more than 10 m below him it isn't, 9 m below it is.
    for (y, picked) in [(3.0, false), (6.0, true)] {
        let mut s = Swinger::new(vec![target(Vec3::new(0.0, y, 6.0), SwingKind::Swing, 5.0)]);
        s.pos.y = 15.0;
        s.view = Vec3::new(0.0, -0.6, 0.8);
        s.tick(false, false, Vec2::ZERO);
        assert_eq!(s.r.swing_target().is_some(), picked, "{} m below", 15.0 - y);
    }
    // Pressed once and held, he swings; once the level takes the target away, he lets go.
    let mut s = Swinger::new(vec![target(Vec3::new(0.0, 6.0, 4.0), SwingKind::Swing, 5.0)]);
    for _ in 0..60 {
        s.tick(true, false, Vec2::ZERO);
    }
    assert_eq!(s.r.state(), MoveState::Swing, "pressed once, swinging");
    s.world.0.clear();
    s.tick(true, false, Vec2::ZERO);
    assert_eq!(s.r.state(), MoveState::Fall, "still swinging on a target that's gone");
}

#[test]
fn the_pick_goes_to_targets_off_to_the_side_and_far_away() {
    // Both 8 m away on the level: the one further off the view's centre wins.
    let ahead = Vec3::new(0.0, 6.0, 8.0);
    let aside = Vec3::new(4.0, 6.0, 6.93);
    let mut s = Swinger::new(vec![target(ahead, SwingKind::Swing, 5.0), target(aside, SwingKind::Swing, 5.0)]);
    s.tick(false, false, Vec2::ZERO);
    assert_eq!(s.r.swing_target(), Some(aside));
    // In the same direction: the further one wins.
    let near = Vec3::new(0.0, 3.0, 4.0);
    let mut s = Swinger::new(vec![target(near, SwingKind::Swing, 5.0), target(ahead, SwingKind::Swing, 5.0)]);
    s.tick(false, false, Vec2::ZERO);
    assert_eq!(s.r.swing_target(), Some(ahead));
}

#[test]
fn a_target_stays_usable_for_its_grace_period_unless_out_of_sight() {
    let t = Tuning::default();
    let spot = Vec3::new(0.0, 6.0, 8.0);
    let mut s = Swinger::new(vec![target(spot, SwingKind::Swing, 5.0)]);
    s.tick(false, false, Vec2::ZERO);
    assert_eq!(s.r.swing_target(), Some(spot));
    // The camera turns away: still usable for the grace period, then not.
    s.view = Vec3::new(0.0, 0.3, -1.0);
    let start = s.time;
    while s.r.swing_target().is_some() && s.time - start < 2.0 {
        s.tick(false, false, Vec2::ZERO);
    }
    assert!((s.time - start - t.swingshot.grace).abs() < 0.02, "usable for {:.2} s", s.time - start);
    // Hidden behind a wall: dropped at once.
    let mut s = Swinger::new(vec![target(spot, SwingKind::Swing, 5.0)]);
    s.tick(false, false, Vec2::ZERO);
    s.world.1 = Some(4.0);
    s.tick(false, false, Vec2::ZERO);
    assert_eq!(s.r.swing_target(), None);
}

#[test]
fn the_hookshot_reels_him_in_then_drops_him() {
    let t = Tuning::default();
    let hook = Vec3::new(0.0, 5.0, 8.0);
    let mut s = Swinger::new(vec![target(hook, SwingKind::Hook, 0.0)]);
    s.tick(false, false, Vec2::ZERO);
    s.tick(true, false, Vec2::ZERO);
    assert_eq!(s.r.state(), MoveState::Hook);
    let start = s.time;
    while s.r.state() == MoveState::Hook && s.time - start < 3.0 {
        s.tick(true, false, Vec2::ZERO);
    }
    assert!(s.saw(|e| *e == Event::HookArrived), "never got there, at {:?}", s.pos);
    assert_eq!(s.r.state(), MoveState::Fall);
    let reach = t.swingshot.hook_length + t.swingshot.hook_hand;
    assert!((s.pos.distance(hook) - reach).abs() < 0.15, "dropped {:.2} m from it", s.pos.distance(hook));
    // Top speed on the way: about the Hookshot's.
    assert!(s.time - start < 1.5);
}

/// Fired from the ground at a Hookshot target closer than its length (plus the hand), he's
/// already there: it ends at once instead of reeling him out into the floor.
#[test]
fn the_hookshot_from_close_by_ends_at_once() {
    let hook = Vec3::new(0.0, 2.5, 1.0);
    let mut s = Swinger::new(vec![target(hook, SwingKind::Hook, 0.0)]);
    s.tick(false, false, Vec2::ZERO);
    assert_eq!(s.r.swing_target(), Some(hook), "the target wasn't picked");
    for _ in 0..24 {
        s.tick(true, false, Vec2::ZERO);
    }
    assert!(s.saw(|e| *e == Event::HookArrived), "never arrived");
    assert_ne!(s.r.state(), MoveState::Hook);
}

#[test]
fn letting_go_of_the_hookshot_mid_reel_keeps_his_upward_speed() {
    let hook = Vec3::new(0.0, 12.0, 6.0);
    let mut s = Swinger::new(vec![target(hook, SwingKind::Hook, 0.0)]);
    s.tick(false, false, Vec2::ZERO);
    for _ in 0..30 {
        s.tick(true, false, Vec2::ZERO);
    }
    assert_eq!(s.r.state(), MoveState::Hook);
    let rising = s.r.velocity().y;
    assert!(rising > 3.0, "reeling up at only {rising} m/s");
    s.tick(false, false, Vec2::ZERO);
    assert_eq!(s.r.state(), MoveState::Fall);
    assert!(s.r.velocity().y > rising * 0.8, "dropped to {} m/s on letting go", s.r.velocity().y);
}

#[test]
fn the_hookshot_arrives_at_the_same_time_at_any_tick_rate() {
    let hook = Vec3::new(0.0, 6.0, 9.0);
    let arrival = |hz: f32| {
        let dt = 1.0 / hz;
        let world = Targets(vec![target(hook, SwingKind::Hook, 0.0)], None);
        let mut r = Ratchet::new(Tuning::default());
        let (mut pos, mut contact, mut time) = (Vec3::ZERO, Contact::default(), 0.0);
        let view = Vec3::new(0.0, 0.3, 1.0).normalize();
        for tick in 0..(4.0 * hz) as usize {
            let input = Input { swingshot: tick > 0, view_forward: view, view_right: Vec2::new(-1.0, 0.0), ..Default::default() };
            let v = r.step_in(dt, &input, &contact, pos, &world);
            if r.events().contains(&Event::HookArrived) {
                return time;
            }
            pos += v * dt;
            time += dt;
            let grounded = pos.y <= 0.0;
            if grounded {
                pos.y = 0.0;
            }
            contact = Contact { grounded, ground_distance: Some(pos.y.max(0.0)), ..Contact::default() };
        }
        f32::INFINITY
    };
    let at = [30.0, 60.0, 120.0].map(arrival);
    assert!(at.iter().all(|a| a.is_finite()), "never arrived: {at:?}");
    assert!(at.iter().all(|a| (a - at[2]).abs() <= 2.0 / 30.0), "arrived at {at:?} s (30, 60, 120 Hz)");
}

#[test]
fn without_the_cameras_right_the_view_still_limits_the_pick() {
    // Facing +Z, the camera looking +X: a target straight along +Z is 90° off the camera's
    // forward, outside the frustum, whether or not the host gives the camera's right.
    let world = Targets(vec![target(Vec3::new(0.0, 6.0, 8.0), SwingKind::Swing, 5.0)], None);
    for right in [Vec2::new(0.0, -1.0), Vec2::ZERO] {
        let mut r = Ratchet::new(Tuning::default());
        let input = Input { view_forward: Vec3::new(1.0, 0.3, 0.0).normalize(), view_right: right, ..Default::default() };
        r.step_in(DT, &input, &Contact::default(), Vec3::ZERO, &world);
        assert_eq!(r.swing_target(), None, "picked a target 90° off the view (camera right {right:?})");
    }
}
