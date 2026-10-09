//! A weapon's trigger, ammo and spread.

use crate::params::{SpreadParams, Trigger, WeaponParams};
use crate::projectile::lob_velocity;
use glam::{Quat, Vec3};

/// Where the shot leaves and what it's aimed at (the host's crosshair hit, or a far point).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub muzzle: Vec3,
    pub target: Vec3,
    /// Aiming (tighter spread, aimed rate of fire) rather than firing from the hip.
    pub aimed: bool,
}

/// A projectile to spawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    pub origin: Vec3,
    pub velocity: Vec3,
}

/// The gun counts as ready this close to the end of its wait (float rounding).
const READY: f32 = 1e-5;

#[derive(Clone, Debug)]
pub struct Weapon {
    params: WeaponParams,
    ammo: u32,
    /// Seconds until the next shot may fire.
    cooldown: f32,
    /// Current spread cone half-angle (degrees) and shots fired in a row.
    spread: f32,
    streak: u32,
    prev_trigger: bool,
    rng: u32,
}

impl Weapon {
    pub fn new(params: WeaponParams) -> Self {
        let ammo = params.ammo;
        let spread = params.spread.initial;
        Weapon { params, ammo, cooldown: 0.0, spread, streak: 0, prev_trigger: false, rng: 0x9E37_79B9 }
    }

    pub fn params(&self) -> &WeaponParams {
        &self.params
    }

    /// Swaps the numbers (e.g. a reload or a size change), keeping ammo and cooldown.
    pub fn set_params(&mut self, params: WeaponParams) {
        self.ammo = self.ammo.min(params.ammo);
        self.params = params;
    }

    pub fn ammo(&self) -> u32 {
        self.ammo
    }

    pub fn refill(&mut self) {
        self.ammo = self.params.ammo;
    }

    /// A cooked shot is winding up: the trigger is held, there's ammo for it and the last one's
    /// wait is over (letting go before then throws nothing).
    pub fn cooking(&self) -> bool {
        self.params.trigger == Trigger::Cooked && self.prev_trigger && self.ammo >= self.params.ammo_cost && self.cooldown <= READY
    }

    /// Advances one tick while the weapon is put away (or can't fire): it fires nothing, but
    /// follows the button, so a press or release made meanwhile isn't taken for one when it's
    /// back (RiftApart.exe reads them from the controller each frame and keeps no memory of
    /// the button in the weapon), and its wait runs down (the game times it from the last
    /// shot) as its spread settles.
    pub fn stow(&mut self, dt: f32, trigger: bool) {
        self.prev_trigger = trigger;
        self.cooldown = (self.cooldown - dt).max(0.0);
        self.streak = 0;
        let spread = self.spread_params(false).clone();
        self.spread = (self.spread - spread.recover * dt).max(spread.initial);
    }

    /// Current spread cone half-angle, degrees.
    pub fn spread(&self) -> f32 {
        self.spread
    }

    /// Advances one tick. Returns the shots fired (at most one per tick at sane rates).
    pub fn step(&mut self, dt: f32, trigger: bool, aim: &Aim) -> Vec<Shot> {
        let pressed = trigger && !self.prev_trigger;
        let released = !trigger && self.prev_trigger;
        self.prev_trigger = trigger;
        let was_ready = self.cooldown <= READY;
        self.cooldown -= dt;
        let spread = self.spread_params(aim.aimed).clone();
        // Letting go ends the streak and narrows the cone back.
        if !trigger {
            self.streak = 0;
            self.spread = (self.spread - spread.recover * dt).max(spread.initial);
        }

        let wants = match self.params.trigger {
            Trigger::Auto => trigger,
            Trigger::Single => pressed,
            Trigger::Cooked => released,
        };
        if !wants || self.cooldown > READY || self.ammo < self.params.ammo_cost {
            // Ready and not firing, the wait doesn't bank up into a burst later.
            self.cooldown = self.cooldown.max(0.0);
            return vec![];
        }
        self.ammo -= self.params.ammo_cost;
        let rate = if aim.aimed { self.params.rate } else { self.params.rapid_rate };
        // A shot that came due part-way through the tick passes the rest of the tick on to the
        // next wait, so the rate of fire doesn't depend on the tick rate; one fired from ready
        // starts its wait now.
        self.cooldown = if was_ready { 0.0 } else { self.cooldown } + 1.0 / rate.max(0.01);
        self.streak += 1;
        if self.streak > spread.free_shots {
            self.spread = (self.spread + spread.per_shot).min(spread.max.max(spread.initial));
        }

        let velocity = if self.params.lob {
            let to = aim.target - aim.muzzle;
            let target = if to.length() > self.params.lob_range { aim.muzzle + to.normalize() * self.params.lob_range } else { aim.target };
            lob_velocity(aim.muzzle, target, self.params.projectile.speed, self.params.projectile.gravity)
        } else {
            let dir = (aim.target - aim.muzzle).normalize_or(Vec3::Z);
            self.scatter(dir, self.spread) * self.params.projectile.speed
        };
        vec![Shot { origin: aim.muzzle, velocity }]
    }

    fn spread_params(&self, aimed: bool) -> &SpreadParams {
        if aimed { &self.params.aimed_spread } else { &self.params.spread }
    }

    /// A random direction within `angle` degrees of `dir` (uniform over the cone's disc).
    fn scatter(&mut self, dir: Vec3, angle: f32) -> Vec3 {
        if angle <= 0.0 {
            return dir;
        }
        let r = angle.to_radians() * self.random().sqrt();
        let around = self.random() * std::f32::consts::TAU;
        let side = dir.any_orthonormal_vector();
        let tilt = Quat::from_axis_angle(Quat::from_axis_angle(dir, around) * side, r);
        (tilt * dir).normalize()
    }

    /// Small deterministic generator (xorshift), so tests and replays are repeatable.
    fn random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}
