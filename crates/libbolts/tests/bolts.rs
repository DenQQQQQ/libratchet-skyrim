//! Checks the bolt rules against the game's numbers (RiftApart.exe, see MODLOG.md): amounts and
//! the split into pickups, the burst, landing and bouncing, the swarm, the caps, and the HUD's
//! tally.

use libbolts::glam::Vec3;
use libbolts::{
    BoltParams, BoltWorld, Cause, Collision, DEFAULT_LEVEL_MULTIPLIER, Hero, Hit, Kind, Rng, State, Tally, Tier, bolt_amount, pickup_budget, split,
};
use std::collections::HashMap;

const DT: f32 = 1.0 / 120.0;

/// An endless floor at y = 0.
fn floor(from: Vec3, to: Vec3) -> Option<Hit> {
    (from.y >= 0.0 && to.y < 0.0).then(|| Hit { point: from.lerp(to, from.y / (from.y - to.y)), normal: Vec3::Y })
}

/// The floor, inside a square pen of walls `half` metres out from the origin each way.
fn pen(half: f32) -> impl Collision {
    move |from: Vec3, to: Vec3| {
        let mut best: Option<(f32, Hit)> = floor(from, to).map(|h| (from.distance(h.point), h));
        for (axis, sign) in [(0, 1.0), (0, -1.0), (2, 1.0), (2, -1.0)] {
            let wall = half * sign;
            let (a, b) = (from[axis] * sign, to[axis] * sign);
            if a < half && b >= half {
                let point = from.lerp(to, (wall - from[axis]) / (to[axis] - from[axis]));
                let mut normal = Vec3::ZERO;
                normal[axis] = -sign;
                let d = from.distance(point);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, Hit { point, normal }));
                }
            }
        }
        best.map(|(_, h)| h)
    }
}

fn nowhere(_: Vec3, _: Vec3) -> Option<Hit> {
    None
}

/// A hero standing at `at`, out of everything's way unless he's close.
fn hero(at: Vec3) -> Hero {
    Hero { position: at, center: at + Vec3::Y * 0.6, wrench: None }
}

fn far() -> Hero {
    hero(Vec3::new(500.0, 0.0, 0.0))
}

/// Steps until `done` or `secs` run out; returns the time it took (None if it never did) and
/// the bolts collected meanwhile.
fn run(w: &mut BoltWorld, secs: f32, hero: Hero, level: &impl Collision, done: impl Fn(&BoltWorld) -> bool) -> (Option<f32>, u32) {
    let mut credited = 0;
    for i in 0..(secs / DT) as usize {
        w.step(DT, &hero, level);
        credited += w.collected().iter().map(|c| c.bolts()).sum::<u32>();
        if done(w) {
            return (Some((i + 1) as f32 * DT), credited);
        }
    }
    (None, credited)
}

fn all_resting(w: &BoltWorld) -> bool {
    w.pickups().iter().all(|p| p.state() == State::Resting)
}

/// Pickups by tier.
fn tally_tiers(kinds: &[Kind]) -> HashMap<Tier, usize> {
    let mut by = HashMap::new();
    for k in kinds {
        *by.entry(k.tier).or_insert(0) += 1;
    }
    by
}

#[test]
fn amounts_and_budgets_at_the_default_multiplier() {
    let lm = DEFAULT_LEVEL_MULTIPLIER;
    for (rv, bolts, budget) in [(1.0, 10, 15), (2.0, 20, 20), (3.0, 30, 22), (5.0, 50, 25), (20.0, 200, 30)] {
        assert_eq!(bolt_amount(rv, lm), bolts, "RelativeValue {rv}");
        assert_eq!(pickup_budget(rv, lm), budget, "RelativeValue {rv}");
    }
    // At least one bolt, and higher multipliers raise the budget up to twice.
    assert_eq!(bolt_amount(0.01, lm), 1);
    assert_eq!(pickup_budget(20.0, 81.66), 54);
    assert_eq!(pickup_budget(20.0, 1000.0), 60);
}

#[test]
fn the_split_matches_the_games_examples() {
    let mut rng = Rng::new(7);
    let lm = DEFAULT_LEVEL_MULTIPLIER;
    let cases: [(f32, &[(Tier, usize)]); 5] = [
        (1.0, &[(Tier::Bronze, 10)]),
        // The test dummy.
        (2.0, &[(Tier::Bronze, 20)]),
        (3.0, &[(Tier::Silver, 4), (Tier::Bronze, 18)]),
        (5.0, &[(Tier::Silver, 13), (Tier::Bronze, 11)]),
        (20.0, &[(Tier::Gold, 16), (Tier::Silver, 13), (Tier::Bronze, 1)]),
    ];
    for (rv, want) in cases {
        let kinds = split(bolt_amount(rv, lm), pickup_budget(rv, lm), &mut rng);
        assert_eq!(tally_tiers(&kinds), want.iter().copied().collect(), "RelativeValue {rv}");
        assert_eq!(kinds.iter().map(|k| k.count()).sum::<u32>(), bolt_amount(rv, lm));
        assert!(kinds.iter().all(|k| k.variant < 3));
    }
    // Every look of a tier turns up.
    let looks: std::collections::HashSet<u8> = split(200, 30, &mut rng).iter().filter(|k| k.tier == Tier::Gold).map(|k| k.variant).collect();
    assert_eq!(looks.len(), 3);
}

#[test]
fn big_rewards_lose_bolts_and_the_last_pickups_can_overpay() {
    let mut rng = Rng::new(3);
    // RelativeValue 20 at the finale's 81.66: 1633 bolts, but 54 pickups hold at most 1620,
    // all platinum, each the first look (none is big enough).
    let kinds = split(bolt_amount(20.0, 81.66), pickup_budget(20.0, 81.66), &mut rng);
    assert_eq!(bolt_amount(20.0, 81.66), 1633);
    assert_eq!(kinds.len(), 54);
    assert!(kinds.iter().all(|k| *k == Kind { tier: Tier::Platinum, variant: 0 }));
    // RelativeValue 10 there: 816 bolts come out as 820.
    let kinds = split(bolt_amount(10.0, 81.66), pickup_budget(10.0, 81.66), &mut rng);
    assert_eq!(kinds.iter().map(|k| k.count()).sum::<u32>(), 820);
}

#[test]
fn the_burst_flies_out_within_the_games_bounds() {
    let p = BoltParams::default();
    let mut w = BoltWorld::new(p.clone(), 11);
    let kinds = [Kind { tier: Tier::Bronze, variant: 0 }; 150];
    w.spawn(Vec3::new(0.0, 1.0, 0.0), &kinds);
    let (mut fastest, mut slowest, mut lowest, mut ways) = (0.0f32, f32::MAX, f32::MAX, [false; 4]);
    for b in w.pickups() {
        let out = b.velocity() - Vec3::Y * 2.0;
        let speed = out.length();
        let elevation = (out.y / speed).asin();
        assert!((3.99..=7.5).contains(&speed), "speed {speed}");
        assert!((0.5 - 1e-4..=std::f32::consts::FRAC_PI_2 + 1e-4).contains(&elevation), "elevation {elevation}");
        assert!((12.0..=18.75).contains(&b.gravity()), "gravity {}", b.gravity());
        (fastest, slowest, lowest) = (fastest.max(speed), slowest.min(speed), lowest.min(elevation));
        ways[((out.x.atan2(out.z) + std::f32::consts::PI) / std::f32::consts::FRAC_PI_2) as usize % 4] = true;
    }
    assert!(fastest > 7.3 && slowest < 4.5 && lowest < 0.6, "the range is used: {slowest}..{fastest} m/s, from {lowest} rad");
    assert!(ways.iter().all(|w| *w), "every way round");
}

#[test]
fn a_drop_on_a_flat_floor_comes_to_rest_and_bobs() {
    let mut w = BoltWorld::new(BoltParams::default(), 5);
    let spawned = w.drop_reward(Vec3::new(0.0, 1.0, 0.0), 5.0, DEFAULT_LEVEL_MULTIPLIER);
    assert_eq!(spawned, 50);
    assert_eq!(w.pickups().len(), 24);
    let (took, credited) = run(&mut w, 5.0, far(), &floor, all_resting);
    let took = took.expect("all at rest");
    assert!(took < 2.0, "took {took} s");
    assert_eq!(credited, 0);
    // On the floor, 0.15 m up, bobbing up to 0.1 m above that.
    let mut highest = 0.0f32;
    for _ in 0..600 {
        w.step(DT, &far(), &floor);
        for b in w.pickups() {
            let y = b.position().y;
            assert!((0.15 - 1e-4..=0.25 + 1e-4).contains(&y), "at {y} m");
            highest = highest.max(y);
        }
    }
    assert!(highest > 0.24, "bobbed to {highest}");
    // Spread out round the spot, a few metres at most.
    let reach = w.pickups().iter().map(|b| b.position().with_y(0.0).length()).fold(0.0, f32::max);
    assert!(reach > 1.0 && reach < 8.0, "spread {reach} m");
}

#[test]
fn walls_bounce_bolts_back_at_half_speed() {
    let level = pen(0.6);
    let mut w = BoltWorld::new(BoltParams::default(), 9);
    w.spawn(Vec3::new(0.0, 1.0, 0.0), &[Kind { tier: Tier::Silver, variant: 1 }; 40]);
    let mut bounced = 0;
    for _ in 0..(3.0 / DT) as usize {
        let before: Vec<(u32, Vec3, Vec3, f32, u32)> = w.pickups().iter().map(|b| (b.id(), b.position(), b.velocity(), b.gravity(), b.bounces())).collect();
        w.step(DT, &far(), &level);
        for b in w.pickups() {
            let (_, p, v, g, n) = *before.iter().find(|x| x.0 == b.id()).unwrap();
            if b.bounces() > n {
                bounced += 1;
                let hit = v - Vec3::Y * g * DT;
                assert!((b.velocity().length() - 0.5 * hit.length()).abs() < 1e-3, "{} after {}", b.velocity().length(), hit.length());
            }
            // No wall lets a bolt through, however slowly it moves into it.
            for axis in [0, 2] {
                assert!(b.position()[axis].abs() < 0.6, "through a wall from {} at {} m/s", p[axis], v[axis]);
            }
        }
    }
    assert!(bounced > 20, "only {bounced} bounces");
    assert!(all_resting(&w));
    let inside = w.pickups().iter().filter(|b| b.position().x.abs() < 0.6 && b.position().z.abs() < 0.6).count();
    assert_eq!(inside, 40, "{inside} of 40 stayed in the pen");
}

/// Dropped right against a wall, steep bursts drift into it slowly; none ends up inside it.
#[test]
fn bolts_dropped_against_a_wall_stay_out_of_it() {
    // Solid from x = 0.1 on.
    let level = |from: Vec3, to: Vec3| {
        let wall = (from.x < 0.1 && to.x >= 0.1).then(|| Hit { point: from.lerp(to, (0.1 - from.x) / (to.x - from.x)), normal: Vec3::NEG_X });
        match (wall, floor(from, to)) {
            (Some(w), Some(f)) => Some(if from.distance(w.point) < from.distance(f.point) { w } else { f }),
            (w, f) => w.or(f),
        }
    };
    for seed in 0..50 {
        let mut w = BoltWorld::new(BoltParams::default(), seed);
        w.drop_reward(Vec3::new(0.0, 1.0, 0.0), 2.0, DEFAULT_LEVEL_MULTIPLIER);
        let (rested, _) = run(&mut w, 4.0, far(), &level, all_resting);
        assert!(rested.is_some(), "seed {seed}: still flying");
        for b in w.pickups() {
            assert!(b.position().x < 0.1, "seed {seed}: a bolt at x {} is in the wall", b.position().x);
        }
    }
}

#[test]
fn the_swarm_collects_every_bolt_and_credits_the_drop() {
    let mut w = BoltWorld::new(BoltParams::default(), 21);
    let spawned = w.drop_reward(Vec3::new(0.0, 1.0, 0.0), 20.0, DEFAULT_LEVEL_MULTIPLIER);
    assert_eq!(spawned, 200);
    // He stands right by the drop: nothing comes while the bolts fly.
    let near = hero(Vec3::new(1.0, 0.0, 0.0));
    w.step(DT, &near, &floor);
    assert!(w.pickups().iter().all(|b| b.state() == State::Flying));
    // He waits off to the side until they land, then walks in.
    run(&mut w, 5.0, hero(Vec3::new(30.0, 0.0, 0.0)), &floor, all_resting);
    let mut credited = 0;
    let mut at = Vec3::new(30.0, 0.0, 0.0);
    let mut time = 0.0;
    while !w.pickups().is_empty() && time < 20.0 {
        at.x = (at.x - 5.0 * DT).max(0.0);
        w.step(DT, &hero(at), &floor);
        credited += w.collected().iter().inspect(|c| assert_eq!(c.cause, Cause::Reached)).map(|c| c.bolts()).sum::<u32>();
        time += DT;
    }
    assert!(w.pickups().is_empty(), "{} left", w.pickups().len());
    assert_eq!(credited, 200);
}

#[test]
fn the_swarm_reaches_a_hero_who_stands_still_quickly() {
    let mut w = BoltWorld::new(BoltParams::default(), 4);
    w.drop_reward(Vec3::new(0.0, 1.0, 0.0), 2.0, DEFAULT_LEVEL_MULTIPLIER);
    let he = hero(Vec3::new(4.0, 0.0, 0.0));
    let (took, credited) = run(&mut w, 6.0, he, &floor, |w| w.pickups().is_empty());
    assert_eq!(credited, 20);
    // Landing takes up to about 1.7 s; the flight to him well under a second more.
    assert!(took.unwrap() < 2.7, "took {took:?}");
}

#[test]
fn the_thrown_wrench_sets_bolts_flying_too() {
    let mut w = BoltWorld::new(BoltParams::default(), 2);
    w.drop_reward(Vec3::new(0.0, 1.0, 0.0), 1.0, DEFAULT_LEVEL_MULTIPLIER);
    run(&mut w, 5.0, far(), &floor, all_resting);
    let he = Hero { wrench: Some(Vec3::new(0.0, 1.0, 0.0)), ..hero(Vec3::new(40.0, 0.0, 0.0)) };
    w.step(DT, &he, &floor);
    assert!(w.pickups().iter().all(|b| b.state() == State::Pulled));
    let (_, credited) = run(&mut w, 10.0, Hero { wrench: None, ..he }, &floor, |w| w.pickups().is_empty());
    assert_eq!(credited, 10);
}

#[test]
fn golden_01_has_a_shorter_range() {
    let p = BoltParams::default();
    assert_eq!(p.range(Kind { tier: Tier::Gold, variant: 0 }), 6.0);
    for (tier, variant) in [(Tier::Gold, 1), (Tier::Gold, 2), (Tier::Bronze, 0), (Tier::Platinum, 2)] {
        assert_eq!(p.range(Kind { tier, variant }), 10.0);
    }
    // 8 m from a resting golden 01 and a silver: only the silver comes.
    let mut w = BoltWorld::new(p, 1);
    w.spawn(Vec3::new(0.0, 1.0, 0.0), &[Kind { tier: Tier::Gold, variant: 0 }, Kind { tier: Tier::Silver, variant: 0 }]);
    run(&mut w, 5.0, far(), &floor, all_resting);
    let spots: Vec<Vec3> = w.pickups().iter().map(|b| b.position()).collect();
    let away = (spots[1] - spots[0]).with_y(0.0).normalize_or(Vec3::X);
    // He stands 8 m from the silver, on the far side from the gold.
    let at = (spots[1] + away * 8.0).with_y(0.0);
    assert!(at.distance(spots[0]) > 8.0);
    w.step(DT, &hero(at), &floor);
    assert_eq!(w.pickups()[0].state(), State::Resting);
    assert_eq!(w.pickups()[1].state(), State::Pulled);
}

#[test]
fn at_most_200_fly_at_once_and_the_rest_wait() {
    let mut w = BoltWorld::new(BoltParams::default(), 8);
    let spot = Vec3::new(0.0, 1.0, 0.0);
    w.spawn(spot, &[Kind { tier: Tier::Bronze, variant: 2 }; 260]);
    let count = |w: &BoltWorld, s: State| w.pickups().iter().filter(|b| b.state() == s).count();
    assert_eq!((count(&w, State::Flying), count(&w, State::Queued)), (200, 60));
    assert!(w.pickups().iter().filter(|b| b.state() == State::Queued).all(|b| b.position() == spot));
    let (took, _) = run(&mut w, 10.0, far(), &floor, |w| {
        assert!(count(w, State::Flying) <= 200);
        all_resting(w)
    });
    assert!(took.is_some());
    assert_eq!(count(&w, State::Resting), 260);
    // Queued bolts can be pulled before they launch.
    let mut w = BoltWorld::new(BoltParams::default(), 8);
    w.spawn(spot, &[Kind { tier: Tier::Bronze, variant: 2 }; 210]);
    w.step(DT, &hero(Vec3::new(3.0, 0.0, 0.0)), &floor);
    assert_eq!((count(&w, State::Pulled), count(&w, State::Flying)), (10, 200));
}

#[test]
fn past_1024_the_farthest_resting_bolt_is_collected() {
    let mut w = BoltWorld::new(BoltParams::default(), 13);
    // Two piles 100 m apart, 1020 bolts; he stands well away from both, nearer the first.
    let he = hero(Vec3::new(-50.0, 0.0, 0.0));
    let silver = Kind { tier: Tier::Silver, variant: 0 };
    w.spawn(Vec3::new(0.0, 1.0, 0.0), &[silver; 520]);
    w.spawn(Vec3::new(0.0, 1.0, 100.0), &[silver; 500]);
    let (_, credited) = run(&mut w, 10.0, he, &floor, all_resting);
    assert_eq!((w.pickups().len(), credited), (1020, 0));
    // 20 more by the first pile: while they fly, the far pile's farthest go, one a tick.
    w.spawn(Vec3::new(0.0, 1.0, 0.0), &[Kind { tier: Tier::Bronze, variant: 1 }; 20]);
    w.step(DT, &he, &floor);
    assert_eq!(w.collected().len(), 1);
    assert_eq!(w.collected()[0].cause, Cause::OverCap);
    let (took, credited) = run(&mut w, 10.0, he, &floor, |w| all_resting(w) && w.pickups().len() <= 1024);
    assert!(took.is_some());
    assert_eq!(w.pickups().len(), 1024);
    assert_eq!(credited, 15 * 3);
    assert_eq!(w.pickups().iter().filter(|b| b.position().z > 50.0).count(), 500 - 16);
}

#[test]
fn bolts_that_fall_out_of_the_level_are_credited() {
    let mut w = BoltWorld::new(BoltParams::default(), 17);
    let spawned = w.drop_reward(Vec3::new(0.0, 1.0, 0.0), 5.0, DEFAULT_LEVEL_MULTIPLIER);
    let mut credited = 0;
    let mut fell = 0;
    for _ in 0..(6.0 / DT) as usize {
        w.step(DT, &far(), &nowhere);
        for c in w.collected() {
            assert_eq!(c.cause, Cause::FellOut);
            credited += c.bolts();
            fell += 1;
        }
        // Nobody still in the level falls at 25 m/s or more.
        assert!(w.pickups().iter().all(|b| b.velocity().y > -25.0));
    }
    assert!(w.pickups().is_empty());
    assert_eq!((credited, fell), (spawned, 24));
}

#[test]
fn a_bigger_hero_gets_a_bigger_burst_on_the_same_clock() {
    let s = 1.55;
    let rest = |params: BoltParams, height: f32| {
        let mut w = BoltWorld::new(params, 99);
        w.drop_reward(Vec3::new(0.0, height, 0.0), 3.0, DEFAULT_LEVEL_MULTIPLIER);
        let (took, _) = run(&mut w, 5.0, far(), &floor, all_resting);
        (took.unwrap(), w.pickups().iter().map(|b| b.position()).collect::<Vec<_>>())
    };
    let (t1, small) = rest(BoltParams::default(), 1.0);
    let (t2, big) = rest(BoltParams::default().scaled(s), s);
    assert!((t1 - t2).abs() <= 2.0 * DT, "{t1} s vs {t2} s");
    for (a, b) in small.iter().zip(&big) {
        assert!((*a * s - *b).length() < 0.02, "{a} x {s} vs {b}");
    }
    let p = BoltParams::default().scaled(s);
    assert!((p.range(Kind { tier: Tier::Bronze, variant: 0 }) - 15.5).abs() < 1e-4);
    assert!((p.rest_lift - 0.2325).abs() < 1e-5);
}

#[test]
fn a_seed_repeats_a_run() {
    let run_once = || {
        let mut w = BoltWorld::new(BoltParams::default(), 42);
        w.drop_reward(Vec3::new(0.0, 1.0, 0.0), 5.0, DEFAULT_LEVEL_MULTIPLIER);
        run(&mut w, 3.0, far(), &pen(2.0), all_resting);
        w.pickups().iter().map(|b| (b.kind(), b.position())).collect::<Vec<_>>()
    };
    assert_eq!(run_once(), run_once());
}

#[test]
fn the_tally_shows_gains_after_two_seconds() {
    let mut t = Tally::new(100);
    assert!(!t.visible());
    let mut clock = 0.0;
    let mut wait = |t: &mut Tally, secs: f32| {
        for _ in 0..(secs / DT).round() as usize {
            t.step(DT);
            clock += DT;
        }
    };
    t.add(5);
    assert_eq!((t.shown(), t.gain(), t.total()), (100, 5, 105));
    wait(&mut t, 1.0);
    t.add(3);
    assert_eq!((t.shown(), t.gain()), (100, 8));
    wait(&mut t, 1.9);
    assert_eq!((t.shown(), t.gain()), (100, 8), "still waiting 1.9 s after the last gain");
    wait(&mut t, 0.2);
    assert_eq!((t.shown(), t.gain()), (108, 0));
    assert!(t.visible());
    // Up 3.5 s after the last change.
    wait(&mut t, 1.3);
    assert!(t.visible());
    wait(&mut t, 0.2);
    assert!(!t.visible());

    // A loss shows at once, a waiting gain with it.
    t.add(10);
    t.spend(20);
    assert_eq!((t.shown(), t.gain(), t.total()), (98, 0, 98));
    // From 0, and to 0, at once too.
    let mut t = Tally::new(0);
    t.add(7);
    assert_eq!((t.shown(), t.gain()), (7, 0));
    t.add(1);
    assert_eq!((t.shown(), t.gain()), (7, 1));
    t.spend(8);
    assert_eq!((t.shown(), t.gain()), (0, 0));
    // Capped.
    let mut t = Tally::new(9_999_990);
    t.add(100);
    assert_eq!(t.total(), 9_999_999);
}
