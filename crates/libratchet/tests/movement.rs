//! Checks the sim against the numbers it was given and against the rules confirmed in
//! RiftApart.exe (see MODLOG.md): apex heights and timing, the double-jump cap, glide limits.

use libratchet::glam::{Vec2, Vec3};
use libratchet::{Contact, Event, Input, MoveState, Ratchet, Tuning};

const DT: f32 = 1.0 / 120.0;

/// Minimal host: an endless flat floor at y = 0, or no floor at all.
struct Flat {
    r: Ratchet,
    pos: Vec3,
    contact: Contact,
    floor: bool,
    time: f32,
    events: Vec<(f32, Event)>,
}

impl Flat {
    fn new(tuning: Tuning) -> Self {
        Flat { r: Ratchet::new(tuning), pos: Vec3::ZERO, contact: Contact::default(), floor: true, time: 0.0, events: vec![] }
    }

    /// No floor: the character starts in the air and falls.
    fn falling(tuning: Tuning) -> Self {
        let mut h = Flat::new(tuning);
        h.floor = false;
        h.contact = Contact { grounded: false, ground_distance: None, hit_ceiling: false };
        h
    }

    fn tick(&mut self, jump: bool, stick: Vec2) {
        self.tick_with(Input { move_dir: stick, jump, dash: false });
    }

    fn tick_with(&mut self, input: Input) {
        let v = self.r.step(DT, &input, &self.contact, self.pos);
        self.events.extend(self.r.events().iter().map(|e| (self.time, *e)));
        self.pos += v * DT;
        self.time += DT;
        let grounded = self.floor && self.pos.y <= 0.0;
        if grounded {
            self.pos.y = 0.0;
        }
        self.contact = Contact {
            grounded,
            ground_distance: self.floor.then_some(self.pos.y.max(0.0)),
            hit_ceiling: false,
        };
    }

    /// Runs `secs` with the button held for the first `hold` seconds; returns (apex, time of apex).
    fn jump(&mut self, hold: f32, secs: f32) -> (f32, f32) {
        let start = self.time;
        let (mut apex, mut apex_t) = (self.pos.y, 0.0);
        while self.time - start < secs {
            self.tick(self.time - start < hold, Vec2::ZERO);
            if self.pos.y > apex {
                apex = self.pos.y;
                apex_t = self.time - start;
            }
        }
        (apex, apex_t)
    }

    /// Presses jump (holding it while `hold`) and runs until the character stops rising.
    /// Returns the peak height and how long it took. Releases the button at the end.
    fn rise(&mut self, hold: bool) -> (f32, f32) {
        let start = self.time;
        self.tick(true, Vec2::ZERO);
        while self.r.velocity().y > 0.0 && self.time - start < 3.0 {
            self.tick(hold, Vec2::ZERO);
        }
        let peak = (self.pos.y, self.time - start);
        self.tick(false, Vec2::ZERO);
        peak
    }

    fn count(&self, pred: impl Fn(&Event) -> bool) -> usize {
        self.events.iter().filter(|(_, e)| pred(e)).count()
    }
}

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn tap_jump_peaks_at_height_min_on_time() {
    // Gravity comes from the tap height, so a tap peaks exactly at TimeToPeak.
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    let (apex, at) = h.jump(DT, 1.5);
    assert!(close(apex, t.single_jump.height_min, 0.02), "apex {apex}");
    assert!(close(at, t.single_jump.time_to_peak, 2.0 * DT), "time to peak {at}");
}

#[test]
fn held_jump_peaks_at_height_max_and_later() {
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    let (apex, at) = h.jump(1.0, 1.5);
    assert!(close(apex, t.single_jump.height_max, 0.02), "apex {apex}");
    assert!(at > t.single_jump.time_to_peak + 0.03, "a held jump should take longer than a tap ({at} s)");
    assert_eq!(h.r.state(), MoveState::Ground, "should have landed");
}

#[test]
fn half_hold_lands_between() {
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    let (apex, _) = h.jump(0.11, 1.5);
    assert!(apex > t.single_jump.height_min + 0.1 && apex < t.single_jump.height_max - 0.1, "apex {apex}");
}

#[test]
fn double_jump_is_capped_but_keeps_its_timing() {
    let t = Tuning::default();
    let cap = t.double_jump.max_total_jump.unwrap();
    let mut h = Flat::new(t.clone());
    let (first, _) = h.rise(true);
    assert!(first + t.double_jump.height_max > cap, "test needs the cap to bite");
    let (apex, at) = h.jump(1.0, 1.5);
    assert_eq!(h.count(|e| *e == Event::DoubleJumped), 1);
    assert!(close(apex, cap, 0.03), "apex {apex}, cap {cap}");
    // Gravity follows the capped height, so it still peaks after TimeToPeak.
    assert!(close(at, t.double_jump.time_to_peak, 2.0 * DT), "double jump peaked after {at} s");
}

#[test]
fn double_jump_past_the_cap_gets_half_a_metre() {
    let t = Tuning::default();
    let cap = t.double_jump.max_total_jump.unwrap();
    let mut h = Flat::new(t.clone());
    // Tap, then get flung upwards (a bounce pad, say) past the double jump's cap.
    h.tick(true, Vec2::ZERO);
    h.tick(false, Vec2::ZERO);
    h.r.set_velocity(Vec3::new(0.0, 12.0, 0.0));
    while h.r.velocity().y > 0.0 {
        h.tick(false, Vec2::ZERO);
    }
    let start = h.pos.y;
    assert!(start > cap, "test needs to be above the cap ({start} m)");
    let (apex, _) = h.jump(1.0, 1.0);
    assert_eq!(h.count(|e| *e == Event::DoubleJumped), 1);
    assert!(close(apex - start, 0.5, 0.03), "double jump above the cap rose {} m", apex - start);
}

#[test]
fn only_one_midair_jump() {
    let t = Tuning::default();
    let mut h = Flat::new(t);
    h.jump(0.25, 0.3);
    for _ in 0..4 {
        h.jump(0.05, 0.1);
    }
    assert_eq!(h.count(|e| *e == Event::DoubleJumped), 1);
}

/// Holds jump for `secs` and returns (fall speed on the tick the glide started, fastest fall
/// during the glide). The game sets the glide's limit from the speed at that moment.
fn glide_speeds(h: &mut Flat, secs: f32) -> (Option<f32>, f32) {
    let mut entry = None;
    let mut fastest: f32 = 0.0;
    for _ in 0..(secs / DT) as usize {
        h.tick(true, Vec2::ZERO);
        if h.r.state() == MoveState::Glide {
            entry.get_or_insert(-h.r.velocity().y);
            fastest = fastest.max(-h.r.velocity().y);
        }
    }
    (entry, fastest)
}

#[test]
fn glide_started_slowly_sinks_at_4() {
    // Walk off a ledge holding jump: the glide starts early, while falling slower than 4 m/s.
    let t = Tuning::default();
    let mut h = Flat::falling(t);
    let (entry, fastest) = glide_speeds(&mut h, 2.0);
    let entry = entry.expect("never glided");
    assert!(entry < 4.0, "entry fall speed {entry}");
    assert!(close(fastest, 4.0, 1e-3), "glide fell at {fastest} m/s");
    assert!(close(-h.r.velocity().y, 4.0, 1e-3));
}

#[test]
fn glide_started_late_keeps_its_speed_up_to_12() {
    let mut t = Tuning::default();
    t.max_midair_jumps = 0; // so the press glides instead of double jumping
    // 0.4 s at 25 m/s² is about 10 m/s (kept); 1 s would be 25 m/s (cut to 12).
    for (fall_secs, expected) in [(0.4, 10.0), (1.0, 12.0)] {
        let mut h = Flat::falling(t.clone());
        for _ in 0..(fall_secs / DT) as usize {
            h.tick(false, Vec2::ZERO);
        }
        let (entry, fastest) = glide_speeds(&mut h, 1.5);
        let entry = entry.expect("never glided");
        assert!(close(entry, expected, 0.3), "glide started at {entry} m/s after {fall_secs} s of falling");
        assert!(close(fastest, entry, 1e-3), "glide sped up from {entry} to {fastest} m/s");
        assert!(close(-h.r.velocity().y, entry, 1e-3));
    }
}

#[test]
fn holding_after_double_jump_glides() {
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    h.rise(true);
    let (entry, fastest) = glide_speeds(&mut h, 3.0);
    let entry = entry.expect("never glided");
    let limit = entry.clamp(4.0, 12.0);
    assert!(fastest <= limit + 1e-3, "glide fell at {fastest} m/s, limit {limit}");
    assert!(close(fastest, limit, 0.05), "glide never reached its limit ({fastest} vs {limit})");
}

#[test]
fn no_glide_right_above_the_floor() {
    let t = Tuning::default();
    let mut h = Flat::new(t);
    // A tap jump peaks at 1 m, below the glide height minimum, so holding again must not glide.
    h.jump(DT, 0.25);
    for _ in 0..60 {
        h.tick(true, Vec2::ZERO);
        assert_ne!(h.r.state(), MoveState::Glide);
    }
}

#[test]
fn walking_off_a_ledge_falls_with_fall_gravity() {
    let t = Tuning::default();
    let mut h = Flat::falling(t.clone());
    for _ in 0..60 {
        h.tick(false, Vec2::ZERO);
    }
    assert_eq!(h.r.state(), MoveState::Fall);
    let expected = t.fall.gravity() * 60.0 * DT;
    assert!(close(-h.r.velocity().y, expected, 0.5), "fall speed {} vs {expected}", -h.r.velocity().y);
    for _ in 0..1200 {
        h.tick(false, Vec2::ZERO);
    }
    assert!(close(-h.r.velocity().y, t.fall.terminal_velocity, 1e-3));
}

#[test]
fn runs_at_run_speed_and_stops() {
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    for _ in 0..120 {
        h.tick(false, Vec2::new(0.0, 1.0));
    }
    let v = h.r.velocity();
    assert!(close(v.z, t.ground_speeds.run_max, 1e-3), "run speed {}", v.z);
    assert!(close(h.r.facing_yaw(), 0.0, 1e-3));
    for _ in 0..120 {
        h.tick(false, Vec2::ZERO);
    }
    assert!(Vec2::new(h.r.velocity().x, h.r.velocity().z).length() < 1e-3);
}

#[test]
fn scaling_scales_heights_but_not_timing() {
    let t = Tuning::default().scaled(1.6);
    let mut h = Flat::new(t.clone());
    let (apex, at) = h.jump(DT, 2.0);
    assert!(close(apex, Tuning::default().single_jump.height_min * 1.6, 0.03), "apex {apex}");
    assert!(close(at, t.single_jump.time_to_peak, 2.0 * DT), "time to peak {at}");
}

/// Uses the user's extracted Rift Apart configs when they exist (set RIFTAPART_CONFIGS or use
/// the default location). Skipped otherwise, since the repo ships no game data.
#[test]
fn loads_riftapart_configs_when_present() {
    let dir = std::env::var("RIFTAPART_CONFIGS").unwrap_or_else(|_| r"G:\programmering\riftapart-decomp\configs".into());
    let dir = std::path::Path::new(&dir);
    if !dir.join("hero_traversal.json").exists() {
        eprintln!("skipping: no configs at {}", dir.display());
        return;
    }
    let t = Tuning::from_riftapart_configs(dir).expect("configs should parse");
    assert!(t.single_jump.height_max > 0.0 && t.single_jump.time_to_peak > 0.0);
    assert!(t.glide.time_to_peak_is_gravity);
    let j = &t.single_jump;
    let (tap, tap_at) = Flat::new(t.clone()).jump(DT, 2.0);
    let (held, held_at) = Flat::new(t.clone()).jump(1.0, 2.0);
    eprintln!(
        "Rift Apart single jump (gravity {:.1} m/s²): tap {tap:.3} m at {tap_at:.3} s, held {held:.3} m at {held_at:.3} s \
         (config: {}-{} m, TimeToPeak {} s)",
        j.gravity(),
        j.height_min,
        j.height_max,
        j.time_to_peak
    );
    assert!(close(tap, j.height_min, 0.02) && close(tap_at, j.time_to_peak, 2.0 * DT));
    assert!(close(held, j.height_max, 0.02));
}

#[test]
fn ground_dash_covers_its_distance_including_braking() {
    let t = Tuning::default();
    let fwd = Vec2::new(0.0, 1.0);
    // From standing and from a full run.
    for run_up in [0, 120] {
        let mut h = Flat::new(t.clone());
        for _ in 0..run_up {
            h.tick(false, fwd);
        }
        let start = h.pos.z;
        h.tick_with(Input { move_dir: fwd, jump: false, dash: true });
        assert_eq!(h.r.state(), MoveState::Dash);
        let mut peak: f32 = 0.0;
        let mut ticks = 1;
        while h.r.state() == MoveState::Dash && ticks < 240 {
            h.tick_with(Input { move_dir: fwd, jump: false, dash: true });
            peak = peak.max(h.r.velocity().z);
            ticks += 1;
        }
        assert_eq!(h.r.state(), MoveState::Ground, "dash never ended");
        let travelled = h.pos.z - start;
        assert!((travelled - t.ground_dash.distance).abs() < 0.3, "dash went {travelled} m (run-up {run_up})");
        assert!(peak <= t.ground_dash.max_speed + 1e-3, "peak {peak}");
        assert!(h.r.velocity().z <= t.ground_speeds.run_max + 1e-3);
        assert_eq!(h.count(|e| *e == Event::Dashed), 1, "holding the button must not re-dash");
        eprintln!("dash: {travelled:.2} m in {:.3} s, peak {peak:.1} m/s (run-up {run_up} ticks)", ticks as f32 * DT);
    }
}

#[test]
fn air_dash_is_flat_and_once_per_air_time() {
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    let fwd = Vec2::new(0.0, 1.0);
    h.rise(true);
    let start = h.pos;
    h.tick_with(Input { move_dir: fwd, jump: false, dash: true });
    assert_eq!(h.r.state(), MoveState::AirDash);
    while h.r.state() == MoveState::AirDash {
        assert!((h.pos.y - start.y).abs() < 0.05, "air dash changed height to {}", h.pos.y);
        h.tick_with(Input { move_dir: fwd, jump: false, dash: false });
    }
    let travelled = h.pos.z - start.z;
    assert!((travelled - t.air_dash.distance).abs() < 0.3, "air dash went {travelled} m");
    // A second press in the same air time does nothing.
    h.tick_with(Input { move_dir: fwd, jump: false, dash: true });
    assert_ne!(h.r.state(), MoveState::AirDash);
    assert_eq!(h.count(|e| *e == Event::AirDashed), 1);
    // After landing it's available again.
    for _ in 0..240 {
        h.tick(false, Vec2::ZERO);
    }
    assert_eq!(h.r.state(), MoveState::Ground);
    h.jump(0.2, 0.25);
    h.tick_with(Input { move_dir: fwd, jump: false, dash: true });
    assert_eq!(h.r.state(), MoveState::AirDash);
}
