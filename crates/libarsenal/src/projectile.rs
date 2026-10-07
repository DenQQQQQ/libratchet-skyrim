//! Shots in flight. The host sweeps each tick's segment against its world.

use crate::params::ProjectileParams;
use crate::weapon::Shot;
use glam::Vec3;

#[derive(Clone, Debug)]
pub struct Projectile {
    params: ProjectileParams,
    position: Vec3,
    velocity: Vec3,
    travelled: f32,
    age: f32,
    spin: f32,
}

impl Projectile {
    pub fn new(shot: &Shot, params: &ProjectileParams) -> Self {
        Projectile { params: params.clone(), position: shot.origin, velocity: shot.velocity, travelled: 0.0, age: 0.0, spin: 0.0 }
    }

    /// Advances one tick under gravity (exact for constant gravity) and returns the segment it
    /// moved along, for the host's collision sweep.
    pub fn step(&mut self, dt: f32) -> (Vec3, Vec3) {
        let from = self.position;
        let g = Vec3::new(0.0, -self.params.gravity, 0.0);
        self.position += self.velocity * dt + 0.5 * g * dt * dt;
        self.velocity += g * dt;
        self.travelled += self.position.distance(from);
        self.age += dt;
        self.spin = (self.spin + self.params.spin.to_radians() * dt) % std::f32::consts::TAU;
        (from, self.position)
    }

    /// Past its range (straight shots) or its lifetime.
    pub fn expired(&self) -> bool {
        (self.params.gravity == 0.0 && self.travelled >= self.params.range) || self.age >= self.params.lifetime
    }

    pub fn position(&self) -> Vec3 {
        self.position
    }

    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    pub fn radius(&self) -> f32 {
        self.params.radius
    }

    /// Tumble angle (radians) for the visual.
    pub fn spin(&self) -> f32 {
        self.spin
    }
}

/// Launch velocity at `speed` that lands on `target` under `gravity`: the lower of the two arcs.
/// Out of reach, it throws at 45° (the longest throw) towards the target.
pub fn lob_velocity(from: Vec3, target: Vec3, speed: f32, gravity: f32) -> Vec3 {
    let d = target - from;
    let flat = glam::Vec2::new(d.x, d.z);
    let dist = flat.length();
    let dir = if dist > 1e-4 { flat / dist } else { glam::Vec2::Y };
    let (v2, g) = (speed * speed, gravity.max(1e-4));
    let disc = v2 * v2 - g * (g * dist * dist + 2.0 * d.y * v2);
    let angle = if disc >= 0.0 && dist > 1e-4 { ((v2 - disc.sqrt()) / (g * dist)).atan() } else { std::f32::consts::FRAC_PI_4 };
    let (s, c) = angle.sin_cos();
    Vec3::new(dir.x * c * speed, s * speed, dir.y * c * speed)
}
