//! Checks the weapon rules against the numbers they're given: fire rates, ammo, spread, flight,
//! lobbed aim and explosion falloff.

use libarsenal::glam::Vec3;
use libarsenal::{Aim, Projectile, Weapon, WeaponParams, lob_velocity};

const DT: f32 = 1.0 / 120.0;

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn aim(aimed: bool) -> Aim {
    Aim { muzzle: Vec3::new(0.0, 1.0, 0.0), target: Vec3::new(0.0, 1.0, 20.0), aimed }
}

/// Shots fired over `secs` with the trigger held.
fn hold(w: &mut Weapon, secs: f32, aimed: bool) -> usize {
    (0..(secs / DT) as usize).map(|_| w.step(DT, true, &aim(aimed)).len()).sum()
}

#[test]
fn auto_fire_follows_its_rates() {
    let p = WeaponParams::pistol();
    let hip = hold(&mut Weapon::new(p.clone()), 2.0, false);
    let aimed = hold(&mut Weapon::new(p.clone()), 2.0, true);
    assert!(close(hip as f32, p.rapid_rate * 2.0, 1.0), "{hip} shots from the hip");
    assert!(close(aimed as f32, p.rate * 2.0, 1.0), "{aimed} shots aimed");
}

#[test]
fn single_shot_needs_a_press_and_waits_out_its_rate() {
    let p = WeaponParams::grenade();
    let mut w = Weapon::new(p.clone());
    assert_eq!(hold(&mut w, 2.0, false), 1, "holding fires once");
    // Two quick presses: the second is inside the cooldown.
    let mut w = Weapon::new(p.clone());
    let a = aim(false);
    let mut shots = w.step(DT, true, &a).len();
    w.step(DT, false, &a);
    shots += w.step(DT, true, &a).len();
    assert_eq!(shots, 1);
    // After the cooldown a press fires again.
    for _ in 0..((1.0 / p.rate / DT) as usize + 2) {
        w.step(DT, false, &a);
    }
    assert_eq!(w.step(DT, true, &a).len(), 1);
}

#[test]
fn ammo_runs_out() {
    let mut p = WeaponParams::pistol();
    p.ammo = 5;
    let mut w = Weapon::new(p);
    assert_eq!(hold(&mut w, 5.0, false), 5);
    assert_eq!(w.ammo(), 0);
    w.refill();
    assert_eq!(w.ammo(), 5);
}

#[test]
fn spread_widens_with_a_streak_and_recovers() {
    let p = WeaponParams::pistol();
    let mut w = Weapon::new(p.clone());
    assert!(close(w.spread(), p.spread.initial, 1e-4));
    hold(&mut w, 3.0, false);
    let widened = w.spread();
    assert!(widened > p.spread.initial + 0.5, "spread only {widened}");
    assert!(widened <= p.spread.max + 1e-4);
    for _ in 0..240 {
        w.step(DT, false, &aim(false));
    }
    assert!(close(w.spread(), p.spread.initial, 1e-3), "recovered to {}", w.spread());
}

#[test]
fn shots_stay_inside_the_cone() {
    let p = WeaponParams::pistol();
    let mut w = Weapon::new(p.clone());
    let a = aim(false);
    for _ in 0..600 {
        let spread = w.spread();
        for s in w.step(DT, true, &a) {
            let off = s.velocity.normalize().angle_between(Vec3::Z).to_degrees();
            assert!(off <= spread + p.spread.per_shot + 1e-3, "shot {off}° off with a {spread}° cone");
        }
    }
}

#[test]
fn straight_shots_fly_at_speed_and_expire_at_range() {
    let p = WeaponParams::pistol();
    let mut w = Weapon::new(p.clone());
    let shot = w.step(DT, true, &aim(false))[0];
    let mut proj = Projectile::new(&shot, &p.projectile);
    let mut t = 0.0;
    while !proj.expired() && t < 10.0 {
        proj.step(DT);
        t += DT;
    }
    let flown = proj.position().distance(shot.origin);
    assert!(close(flown, p.projectile.range, p.projectile.speed * DT + 0.01), "flew {flown} m");
    assert!(close(t, p.projectile.range / p.projectile.speed, 2.0 * DT), "took {t} s");
}

/// Flies a lobbed shot until it comes back down to the target's height.
fn landing(from: Vec3, target: Vec3, speed: f32, gravity: f32) -> Vec3 {
    let p = libarsenal::ProjectileParams { speed, gravity, range: 1000.0, lifetime: 30.0, radius: 0.1, spin: 0.0 };
    let shot = libarsenal::Shot { origin: from, velocity: lob_velocity(from, target, speed, gravity) };
    let mut proj = Projectile::new(&shot, &p);
    loop {
        let (a, b) = proj.step(DT / 4.0);
        if proj.velocity().y < 0.0 && b.y <= target.y {
            let t = (a.y - target.y) / (a.y - b.y);
            return a + (b - a) * t;
        }
    }
}

#[test]
fn lobbed_shots_land_on_the_target() {
    let from = Vec3::new(0.0, 1.2, 0.0);
    for target in [Vec3::new(0.0, 0.0, 8.0), Vec3::new(12.0, 0.0, -5.0), Vec3::new(3.0, 2.0, 15.0)] {
        let at = landing(from, target, 25.0, 25.0);
        assert!(at.distance(target) < 0.15, "aimed at {target}, landed at {at}");
    }
}

#[test]
fn lobbed_shots_out_of_reach_go_as_far_as_they_can() {
    let (v, g) = (20.0, 20.0);
    let from = Vec3::ZERO;
    let at = landing(from, Vec3::new(0.0, 0.0, 100.0), v, g);
    assert!(close(at.z, v * v / g, 0.2), "45° throw went {} m", at.z);
}

#[test]
fn grenades_aim_no_further_than_their_range() {
    let p = WeaponParams::grenade();
    let mut w = Weapon::new(p.clone());
    let a = Aim { muzzle: Vec3::ZERO, target: Vec3::new(0.0, 0.0, 80.0), aimed: false };
    let shot = w.step(DT, true, &a)[0];
    let at = landing(shot.origin, Vec3::new(0.0, 0.0, p.lob_range), p.projectile.speed, p.projectile.gravity);
    let real = {
        let pp = libarsenal::ProjectileParams { lifetime: 30.0, ..p.projectile.clone() };
        let mut proj = Projectile::new(&shot, &pp);
        loop {
            let (a, b) = proj.step(DT / 4.0);
            if proj.velocity().y < 0.0 && b.y <= 0.0 {
                break a + (b - a) * (a.y / (a.y - b.y));
            }
        }
    };
    assert!(real.distance(at) < 0.2 && close(real.z, p.lob_range, 0.2), "landed at {real}");
}

#[test]
fn cooked_shots_wind_up_while_held_and_leave_on_release() {
    let mut p = WeaponParams::grenade();
    p.trigger = libarsenal::Trigger::Cooked;
    let mut w = Weapon::new(p);
    let a = Aim { muzzle: Vec3::ZERO, target: Vec3::new(0.0, 0.0, 10.0), aimed: false };
    for _ in 0..30 {
        assert!(w.step(DT, true, &a).is_empty());
        assert!(w.cooking());
    }
    assert_eq!(w.step(DT, false, &a).len(), 1);
    assert!(!w.cooking());
    assert!(w.step(DT, false, &a).is_empty());
}

/// Put away part-way through a wind-up, the weapon follows the button: let go meanwhile and
/// nothing is thrown when it's back; held all through, it winds up again and throws on
/// release.
#[test]
fn a_cooked_throw_put_away_isnt_thrown_when_it_comes_back() {
    let mut p = WeaponParams::grenade();
    p.trigger = libarsenal::Trigger::Cooked;
    let a = Aim { muzzle: Vec3::ZERO, target: Vec3::new(0.0, 0.0, 10.0), aimed: false };
    let mut w = Weapon::new(p.clone());
    for _ in 0..30 {
        w.step(DT, true, &a);
    }
    w.stow(DT, true);
    w.stow(DT, false);
    assert!(w.step(DT, false, &a).is_empty(), "thrown on coming back");
    assert!(!w.cooking());
    let mut w = Weapon::new(p);
    for _ in 0..30 {
        w.step(DT, true, &a);
    }
    for _ in 0..30 {
        w.stow(DT, true);
    }
    assert!(w.step(DT, true, &a).is_empty());
    assert!(w.cooking());
    assert_eq!(w.step(DT, false, &a).len(), 1);
}

/// Put away, a weapon's wait after a shot still runs down.
#[test]
fn a_weapon_put_away_waits_out_its_rate() {
    let mut w = Weapon::new(WeaponParams::pistol());
    assert_eq!(w.step(DT, true, &aim(true)).len(), 1);
    assert!(w.step(DT, false, &aim(true)).is_empty());
    let wait = 1.0 / w.params().rate;
    for _ in 0..(wait / DT).ceil() as usize {
        w.stow(DT, false);
    }
    assert_eq!(w.step(DT, true, &aim(true)).len(), 1, "still waiting after {wait:.2} s put away");
}

#[test]
fn explosions_fall_off_with_distance() {
    let e = WeaponParams::grenade().explosion.unwrap();
    assert_eq!(e.damage_at(0.5), e.inner_damage);
    let mid = e.damage_at((e.inner_radius + e.outer_radius) / 2.0);
    assert!(close(mid, (e.inner_damage + e.outer_damage) / 2.0, 0.01));
    assert_eq!(e.damage_at(e.outer_radius + 0.1), 0.0);
    assert!(e.knockback_at(0.5) > e.knockback_at(4.0) && e.knockback_at(4.0) > e.knockback_at(10.0));
    assert_eq!(e.knockback_at(100.0), 0.0);
}

#[test]
fn scaling_scales_distances_not_damage_or_rate() {
    let p = WeaponParams::grenade();
    let s = p.scaled(2.0);
    assert_eq!(s.rate, p.rate);
    assert_eq!(s.explosion.as_ref().unwrap().inner_damage, p.explosion.as_ref().unwrap().inner_damage);
    assert_eq!(s.projectile.speed, p.projectile.speed * 2.0);
    assert_eq!(s.lob_range, p.lob_range * 2.0);
}

#[test]
fn loads_riftapart_weapon_configs_when_present() {
    let dir = std::env::var("RIFTAPART_CONFIGS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../riftapart-configs"));
    let pistol = dir.join("wpn_burstpistol_weapon.json");
    let grenade = dir.join("wpn_protogrenade_weapon.json");
    if !pistol.exists() || !grenade.exists() {
        eprintln!("skipping: no weapon configs in {}", dir.display());
        return;
    }
    let bp = WeaponParams::from_riftapart_config(&pistol, &WeaponParams::pistol()).expect("burst pistol");
    let sb = WeaponParams::from_riftapart_config(&grenade, &WeaponParams::grenade()).expect("grenade");
    eprintln!("{bp:#?}\n{sb:#?}");
    assert_eq!(bp.trigger, libarsenal::Trigger::Auto);
    assert!(!bp.lob && bp.projectile.gravity == 0.0 && bp.explosion.is_none());
    assert!(sb.lob && sb.explosion.is_some() && sb.projectile.gravity > 0.0);
    assert_eq!(sb.trigger, libarsenal::Trigger::Cooked);
    // The level numbers win over the base ones.
    assert!(bp.ammo > 60 && bp.damage > 1.0);
    // The pistol throws out shells; the grenade doesn't.
    let shell = bp.shell.as_ref().expect("burst pistol shells");
    assert!(shell.speed.0 > 0.0 && shell.speed.1 >= shell.speed.0);
    assert!(sb.shell.is_none());
    // Camera shakes: the pistol's recoil, the grenade's explosion with its falloff radii.
    assert!(bp.recoil_shake.as_ref().is_some_and(|s| !s.config.contains('/') && !s.config.ends_with(".config")));
    let blast = sb.blast_shake.as_ref().expect("explosion shake");
    assert!(blast.inner > 0.0 && blast.outer >= blast.inner);
    let half = sb.scaled(0.5);
    assert_eq!(half.blast_shake.as_ref().unwrap().inner, blast.inner * 0.5);
}

#[test]
fn the_rate_of_fire_does_not_depend_on_the_tick_rate() {
    // A shot due part-way through a tick passes the rest on to the next wait.
    let p = WeaponParams::pistol();
    for hz in [30.0f32, 60.0, 120.0, 144.0] {
        let dt = 1.0 / hz;
        let mut w = Weapon::new(p.clone());
        let shots: usize = (0..(4.0 * hz) as usize).map(|_| w.step(dt, true, &aim(true)).len()).sum();
        assert!(close(shots as f32, p.rate * 4.0, 1.0), "{shots} shots in 4 s at {hz} Hz (rate {})", p.rate);
    }
}

#[test]
fn a_cooked_shot_does_not_wind_up_until_the_last_ones_wait_is_over() {
    let mut p = WeaponParams::grenade();
    p.trigger = libarsenal::Trigger::Cooked;
    let mut w = Weapon::new(p.clone());
    let a = aim(false);
    w.step(DT, true, &a);
    assert_eq!(w.step(DT, false, &a).len(), 1, "the first throw");
    w.step(DT, true, &a);
    assert!(!w.cooking(), "winding up during the wait");
    for _ in 0..((1.0 / p.rate / DT) as usize + 2) {
        w.step(DT, true, &a);
    }
    assert!(w.cooking());
    assert_eq!(w.step(DT, false, &a).len(), 1, "the second throw");
}
