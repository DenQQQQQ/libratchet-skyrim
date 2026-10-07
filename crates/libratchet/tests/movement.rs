//! Checks the sim against the numbers it was given and against the rules confirmed in
//! RiftApart.exe (see MODLOG.md): apex heights and timing, the double-jump cap, glide limits.

use libratchet::glam::{Vec2, Vec3};
use libratchet::{Contact, Event, Input, MoveState, Ratchet, ThrowStatus, Tuning, WrenchThrow};

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
        self.tick_with(Input { move_dir: stick, jump, dash: false, melee: false, aim: None, aim_mode: false });
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
        h.tick_with(Input { move_dir: fwd, jump: false, dash: true, melee: false, aim: None, aim_mode: false });
        assert_eq!(h.r.state(), MoveState::Dash);
        let mut peak: f32 = 0.0;
        let mut ticks = 1;
        while h.r.state() == MoveState::Dash && ticks < 240 {
            h.tick_with(Input { move_dir: fwd, jump: false, dash: true, melee: false, aim: None, aim_mode: false });
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
    h.tick_with(Input { move_dir: fwd, jump: false, dash: true, melee: false, aim: None, aim_mode: false });
    assert_eq!(h.r.state(), MoveState::AirDash);
    while h.r.state() == MoveState::AirDash {
        assert!((h.pos.y - start.y).abs() < 0.05, "air dash changed height to {}", h.pos.y);
        h.tick_with(Input { move_dir: fwd, jump: false, dash: false, melee: false, aim: None, aim_mode: false });
    }
    let travelled = h.pos.z - start.z;
    assert!((travelled - t.air_dash.distance).abs() < 0.3, "air dash went {travelled} m");
    // A second press in the same air time does nothing.
    h.tick_with(Input { move_dir: fwd, jump: false, dash: true, melee: false, aim: None, aim_mode: false });
    assert_ne!(h.r.state(), MoveState::AirDash);
    assert_eq!(h.count(|e| *e == Event::AirDashed), 1);
    // After landing it's available again.
    for _ in 0..240 {
        h.tick(false, Vec2::ZERO);
    }
    assert_eq!(h.r.state(), MoveState::Ground);
    h.jump(0.2, 0.25);
    h.tick_with(Input { move_dir: fwd, jump: false, dash: true, melee: false, aim: None, aim_mode: false });
    assert_eq!(h.r.state(), MoveState::AirDash);
}

fn melee_press() -> Input {
    Input { melee: true, ..Default::default() }
}

/// Runs one swing from a press to its end; returns how far it lunged (facing +Z).
fn one_swing(h: &mut Flat) -> f32 {
    let start = h.pos;
    h.tick_with(melee_press());
    while h.r.state() == MoveState::Melee && h.time < 10.0 {
        h.tick(false, Vec2::ZERO);
    }
    h.pos.z - start.z
}

#[test]
fn a_swing_lunges_by_its_root_motion_and_hits_once() {
    let t = Tuning::default();
    let w = t.melee.combo[0].clone();
    let mut h = Flat::new(t.clone());
    let lunged = one_swing(&mut h);
    assert!(close(lunged, *w.lunge.last().unwrap(), 0.03), "lunged {lunged} m");
    assert!(close(h.time, w.exit, 2.0 * DT), "swing lasted {} s", h.time);
    let hits: Vec<f32> = h.events.iter().filter(|(_, e)| matches!(e, Event::MeleeHit { index: 0 })).map(|(t, _)| *t).collect();
    assert_eq!(hits.len(), 1);
    assert!(close(hits[0], w.hit_time, 2.0 * DT), "hit at {} s", hits[0]);
    assert_eq!(h.r.state(), MoveState::Ground);
}

/// Swing starts as (time, index).
fn swing_starts(h: &Flat) -> Vec<(f32, usize)> {
    h.events.iter().filter_map(|(t, e)| if let Event::MeleeSwing { index } = e { Some((*t, *index)) } else { None }).collect()
}

#[test]
fn mashing_melee_runs_the_combo_in_order_and_rests_before_starting_over() {
    let t = Tuning::default();
    let m = t.melee.clone();
    let mut h = Flat::new(t);
    for i in 0..480 {
        // A press every 0.1 s.
        h.tick_with(Input { melee: i % 12 == 0, ..Default::default() });
    }
    let starts = swing_starts(&h);
    let order: Vec<usize> = starts.iter().map(|s| s.1).collect();
    assert!(order.len() >= 4, "swings {order:?}");
    assert_eq!(&order[..4], &[0, 1, 2, 0]);
    // Each swing hands over at its first AllowEarlyTransition.
    assert!(close(starts[1].0 - starts[0].0, m.combo[0].early_exit, 2.0 * DT), "second swing at {:?}", starts[1]);
    assert!(close(starts[2].0 - starts[1].0, m.combo[1].early_exit, 2.0 * DT), "third swing at {:?}", starts[2]);
    // The last swing plays out, and the rest it started at its second window runs out first.
    let gap = starts[3].0 - starts[2].0;
    assert!(close(gap, m.combo[2].exit + m.combo_cooldown, 3.0 * DT), "started over after {gap} s");
}

#[test]
fn a_press_soon_after_a_swing_goes_on_with_the_combo() {
    let t = Tuning::default();
    let m = t.melee.clone();
    for (wait, expected) in [(m.combo_timeout * 0.5, 1), (m.combo_timeout + 0.1, 0)] {
        let mut h = Flat::new(t.clone());
        one_swing(&mut h);
        let end = h.time;
        while h.time - end < wait {
            h.tick(false, Vec2::ZERO);
        }
        h.tick_with(melee_press());
        assert_eq!(swing_starts(&h).last().map(|s| s.1), Some(expected), "press {wait} s after the swing");
    }
}

#[test]
fn moving_cuts_a_swing_at_its_early_exit_unless_a_press_waits() {
    let t = Tuning::default();
    let w = t.melee.combo[0].clone();
    let mut h = Flat::new(t.clone());
    h.tick_with(melee_press());
    let mut left = None;
    while h.time < w.duration && left.is_none() {
        h.tick(false, Vec2::new(1.0, 0.0));
        if h.r.state() != MoveState::Melee {
            left = Some(h.time);
        }
    }
    let left = left.expect("moving should cut the swing short");
    assert!(close(left, w.early_exit, 2.0 * DT), "cut at {left} s");

    // With the next swing asked for, the stick doesn't cut the swing: the next one starts.
    let mut h = Flat::new(t.clone());
    h.tick_with(melee_press());
    h.tick(false, Vec2::ZERO);
    h.tick_with(melee_press());
    while h.time < w.exit + 4.0 * DT {
        h.tick(false, Vec2::new(1.0, 0.0));
    }
    let starts = swing_starts(&h);
    assert_eq!(starts.len(), 2, "swings {starts:?}");
    assert!(close(starts[1].0, w.early_exit, 2.0 * DT), "second swing at {} s", starts[1].0);
}

#[test]
fn a_jump_cuts_a_swing_any_time_and_a_dash_never_does() {
    let t = Tuning::default();
    let w = t.melee.combo[0].clone();
    let mut h = Flat::new(t.clone());
    h.tick_with(melee_press());
    h.tick(false, Vec2::ZERO);
    assert!(h.time < w.hit_time);
    h.tick(true, Vec2::ZERO);
    assert_eq!(h.r.state(), MoveState::Jump);

    let mut h = Flat::new(t);
    h.tick_with(melee_press());
    while h.time < w.exit - 2.0 * DT {
        let dash = h.time > w.early_exit && h.time < w.early_exit + 2.0 * DT;
        h.tick_with(Input { dash, ..Default::default() });
        assert_eq!(h.r.state(), MoveState::Melee, "left the swing at {} s", h.time);
    }
}

#[test]
fn hyper_strike_hops_then_slams_down() {
    let t = Tuning::default();
    let m = t.melee.clone();
    let mut h = Flat::new(t.clone());
    h.rise(true);
    let (y0, start) = (h.pos.y, h.time);
    h.tick_with(melee_press());
    assert_eq!(h.r.state(), MoveState::Smash);
    let mut top = h.pos.y;
    while h.r.state() == MoveState::Smash && h.time - start < 3.0 {
        h.tick(false, Vec2::ZERO);
        top = top.max(h.pos.y);
    }
    assert!(close(top - y0, m.smash_hop.height_min, 0.05), "hopped {} m", top - y0);
    assert_eq!(h.r.state(), MoveState::SmashLand);
    let impact = h.events.iter().find_map(|(_, e)| if let Event::SmashImpact { fall_speed } = e { Some(*fall_speed) } else { None });
    assert!(impact.expect("no impact event") > 20.0, "slammed at only {impact:?} m/s");
    let airborne = h.time - start;
    assert!(airborne < m.smash_hop.time_to_peak + 0.25, "took {airborne} s to come down");
    for _ in 0..((m.smash_land_time / DT) as usize + 2) {
        h.tick(false, Vec2::ZERO);
    }
    assert_eq!(h.r.state(), MoveState::Ground);
}

#[test]
fn no_hyper_strike_right_above_the_floor() {
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    h.tick(true, Vec2::ZERO);
    h.tick(false, Vec2::ZERO);
    assert!(h.pos.y > 0.0 && h.pos.y < t.melee.smash_min_height);
    h.tick_with(melee_press());
    assert_ne!(h.r.state(), MoveState::Smash);
}

#[test]
fn thrown_wrench_goes_out_and_comes_back() {
    let p = Tuning::default().melee.throw;
    let mut w = WrenchThrow::new(&p, Vec3::ZERO, Vec3::Z);
    let (mut far, mut t) = (0.0f32, 0.0);
    while w.step(DT, Vec3::ZERO, false) == ThrowStatus::Flying {
        t += DT;
        far = far.max(w.position().z);
        assert!(t < 5.0, "never came back");
    }
    assert!(close(far, p.max_distance, 0.01), "went {far} m");
    let out = p.max_distance / p.out_speed;
    assert!(t > out && t < out + 2.5, "round trip took {t} s");
}

#[test]
fn thrown_wrench_turns_back_when_it_hits_something() {
    let p = Tuning::default().melee.throw;
    let mut w = WrenchThrow::new(&p, Vec3::ZERO, Vec3::X);
    for _ in 0..10 {
        w.step(DT, Vec3::ZERO, false);
    }
    w.step(DT, Vec3::ZERO, true);
    assert!(w.returning());
    assert!(w.position().x < p.max_distance * 0.5);
}

#[test]
fn loads_wrench_configs_when_present() {
    let dir = std::env::var("RIFTAPART_CONFIGS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../riftapart-configs"));
    if !dir.join("hero_melee_clips.json").exists() {
        eprintln!("skipping: no hero_melee_clips.json in {}", dir.display());
        return;
    }
    let t = Tuning::from_riftapart_configs(&dir).expect("configs should parse");
    let m = &t.melee;
    for (i, w) in m.combo.iter().enumerate() {
        eprintln!(
            "swing {}: {:.2} s, hit at {:.2} s, lunge {:.2} m, damage {}, knockback {} m",
            i + 1,
            w.duration,
            w.hit_time,
            w.lunge.last().copied().unwrap_or(0.0),
            w.damage,
            w.knockback
        );
        assert!(w.hit_time > 0.0 && w.hit_time < w.duration && !w.lunge.is_empty());
    }
    eprintln!(
        "hyper-strike: hop {} m in {} s, then {} m/s² (min height {} m), lands for {:.2} s, {} damage in {} m; \
         throw {} m/s out to {} m, back at up to {} m/s",
        m.smash_hop.height_min,
        m.smash_hop.time_to_peak,
        m.smash_gravity,
        m.smash_min_height,
        m.smash_land_time,
        m.smash_damage,
        m.smash_radius,
        m.throw.out_speed,
        m.throw.max_distance,
        m.throw.return_max_speed
    );
    assert_eq!(m.combo.len(), 3);
    assert!(m.smash_gravity > 100.0);
    let mut h = Flat::new(t.clone());
    let lunged = one_swing(&mut h);
    assert!(close(lunged, *m.combo[0].lunge.last().unwrap(), 0.03), "lunged {lunged} m");
}

fn aim_input(stick: Vec2, aim: Vec2, jump: bool) -> Input {
    Input { move_dir: stick, jump, aim: Some(aim), aim_mode: true, ..Default::default() }
}

/// Seconds until the facing is within `tolerance` of `target` (radians).
fn time_to_face(h: &mut Flat, target: f32, tolerance: f32, input: Input) -> f32 {
    let start = h.time;
    while h.time - start < 3.0 {
        h.tick_with(input);
        let d = (h.r.facing_yaw() - target + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        if d.abs() < tolerance {
            return h.time - start;
        }
    }
    f32::MAX
}

#[test]
fn running_turns_around_with_the_games_mover_turn() {
    let mut h = Flat::new(Tuning::default());
    for _ in 0..120 {
        h.tick(false, Vec2::new(0.0, 1.0));
    }
    let back = Input { move_dir: Vec2::new(0.0, -1.0), ..Default::default() };
    let t = time_to_face(&mut h, std::f32::consts::PI, 0.3, back);
    assert!(t > 0.15 && t < 0.45, "turned around in {t} s");
}

#[test]
fn firing_from_the_hip_strafes_at_run_speed_and_aiming_turns_almost_at_once() {
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    let hip = Input { move_dir: Vec2::new(1.0, 0.0), aim: Some(Vec2::new(0.0, 1.0)), ..Default::default() };
    for _ in 0..240 {
        h.tick_with(hip);
    }
    assert!(h.r.facing_yaw().abs() < 0.02, "facing {}", h.r.facing_yaw());
    assert!(close(h.r.velocity().x, t.ground_speeds.run_max, 0.05), "strafing at {}", h.r.velocity());

    let mut h = Flat::new(t);
    let behind = Input { aim: Some(Vec2::new(0.0, -1.0)), aim_mode: true, ..Default::default() };
    let turned = time_to_face(&mut h, std::f32::consts::PI, 0.05, behind);
    assert!(turned < 0.15, "turned in {turned} s");
}

#[test]
fn aiming_faces_the_aim_while_strafing() {
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    let aim = Vec2::new(0.0, 1.0); // facing +Z
    for _ in 0..240 {
        h.tick_with(aim_input(Vec2::new(1.0, 0.0), aim, false)); // moving +X
    }
    assert!(h.r.facing_yaw().abs() < 0.02, "facing {}", h.r.facing_yaw());
    let v = h.r.velocity();
    assert!(close(v.x, t.aim_speeds.run_max, 0.05) && v.z.abs() < 0.05, "strafing at {v}");
}

#[test]
fn aiming_backs_up_slower() {
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    let aim = Vec2::new(0.0, 1.0);
    for _ in 0..240 {
        h.tick_with(aim_input(Vec2::new(0.0, -1.0), aim, false));
    }
    assert!(close(-h.r.velocity().z, t.aim_back_speed, 0.05), "backing at {}", h.r.velocity().z);
}

#[test]
fn aimed_jumps_use_the_aim_mode_heights() {
    let t = Tuning::default();
    let mut h = Flat::new(t.clone());
    let aim = Vec2::new(0.0, 1.0);
    let mut apex: f32 = 0.0;
    for i in 0..180 {
        h.tick_with(aim_input(Vec2::ZERO, aim, i < 60));
        apex = apex.max(h.pos.y);
    }
    assert!(close(apex, t.aim_single_jump.height_max, 0.02), "aimed jump peaked at {apex}");
}

#[test]
fn a_dash_while_strafing_keeps_facing_the_aim() {
    let (ahead, right) = (Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0));
    // Aiming ahead, a dash to the right: he keeps facing the aim and goes sideways.
    let mut h = Flat::new(Tuning::default());
    for _ in 0..30 {
        h.tick_with(aim_input(Vec2::ZERO, ahead, false));
    }
    let start = h.pos;
    h.tick_with(Input { dash: true, ..aim_input(right, ahead, false) });
    assert_eq!(h.r.state(), MoveState::Dash);
    assert!(h.r.dash_strafing());
    while h.r.state() == MoveState::Dash {
        assert!(h.r.facing_yaw().abs() < 1e-3, "turned to {}", h.r.facing_yaw());
        h.tick_with(aim_input(right, ahead, false));
    }
    assert!(h.pos.x - start.x < -5.0, "went {:?}", h.pos - start);
    // Not aiming, the same dash turns him to face where it goes.
    let mut h = Flat::new(Tuning::default());
    h.tick_with(Input { move_dir: right, dash: true, ..Default::default() });
    assert!(!h.r.dash_strafing());
    assert!(close(h.r.facing_yaw(), right.x.atan2(right.y), 1e-3));
}

#[test]
fn a_dash_can_be_jumped_out_of_from_a_tenth_of_a_second() {
    let fwd = Vec2::new(0.0, 1.0);
    let mut h = Flat::new(Tuning::default());
    h.tick_with(Input { move_dir: fwd, dash: true, ..Default::default() });
    for _ in 0..5 {
        h.tick(false, fwd);
    }
    // Pressed 0.05 s in, the jump waits for the dash to allow it at 0.1 s.
    h.tick(true, fwd);
    assert_eq!(h.r.state(), MoveState::Dash, "jumped out 0.05 s in");
    while h.r.state() == MoveState::Dash && h.time < 0.3 {
        h.tick(false, fwd);
    }
    assert_eq!(h.r.state(), MoveState::Jump);
    assert!((h.time - 0.1).abs() <= 2.0 * DT, "jumped at {}", h.time);
}

#[test]
fn a_jump_out_of_a_dash_keeps_its_speed_for_one_tick_only() {
    let t = Tuning::default();
    let fwd = Vec2::new(0.0, 1.0);
    // A running jump, for comparison.
    let mut run = Flat::new(t.clone());
    for _ in 0..240 {
        run.tick(false, fwd);
    }
    let start = run.pos.z;
    run.tick(true, fwd);
    while run.r.state() != MoveState::Ground {
        run.tick(false, fwd);
    }
    let running_jump = run.pos.z - start;
    // Out of a dash: the first jump tick still has the dash's speed, the next the jump's.
    let mut h = Flat::new(t.clone());
    for _ in 0..240 {
        h.tick(false, fwd);
    }
    h.tick_with(Input { move_dir: fwd, dash: true, ..Default::default() });
    while h.time < 2.0 + 0.15 {
        h.tick(false, fwd);
    }
    let dash_speed = h.r.velocity().z;
    assert!(dash_speed > t.air_speeds.run_max * 2.0, "dash at {dash_speed}");
    let start = h.pos.z;
    h.tick(true, fwd);
    assert_eq!(h.r.state(), MoveState::Jump);
    h.tick(false, fwd);
    assert!(close(h.r.velocity().z, dash_speed, 0.5), "first tick {} (dash {dash_speed})", h.r.velocity().z);
    h.tick(false, fwd);
    assert!(h.r.velocity().z < t.air_speeds.run_max + 0.5, "second tick {}", h.r.velocity().z);
    while h.r.state() != MoveState::Ground {
        h.tick(false, fwd);
    }
    let dash_jump = h.pos.z - start;
    assert!(dash_jump < running_jump + 0.6, "dash jump {dash_jump:.2} m, running jump {running_jump:.2} m");
}

#[test]
fn letting_go_of_the_stick_in_a_jump_stops_it_but_a_double_jump_drifts() {
    let t = Tuning::default();
    let fwd = Vec2::new(0.0, 1.0);
    let mut h = Flat::new(t.clone());
    for _ in 0..240 {
        h.tick(false, fwd);
    }
    h.tick(true, fwd);
    for _ in 0..6 {
        h.tick(true, fwd);
    }
    // Steered, then let go: a single jump keeps almost none of its speed.
    for _ in 0..3 {
        h.tick(true, Vec2::ZERO);
    }
    assert!(h.r.velocity().z < 0.5, "single jump still at {}", h.r.velocity().z);
    // A double jump steered forward, then let go: it keeps more for longer.
    let mut h = Flat::new(t.clone());
    h.rise(true);
    h.tick(true, fwd);
    assert_eq!(h.r.state(), MoveState::DoubleJump);
    for _ in 0..12 {
        h.tick(true, fwd);
    }
    h.tick(true, Vec2::ZERO);
    assert!(h.r.velocity().z > 1.0, "double jump stopped at once: {}", h.r.velocity().z);
}
