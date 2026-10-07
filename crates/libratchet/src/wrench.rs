//! The OmniWrench throw: the wrench flies out along the aim, then comes back to the hand.
//!
//! The host owns collision and the hand: each tick it says whether the wrench hit something and
//! where the hand is, and draws the wrench at [`WrenchThrow::position`], spinning by
//! [`WrenchThrow::spin`].

use crate::tuning::ThrowParams;
use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrowStatus {
    Flying,
    /// Back in the hand; the host can drop the throw.
    Caught,
}

#[derive(Clone, Debug)]
pub struct WrenchThrow {
    params: ThrowParams,
    position: Vec3,
    velocity: Vec3,
    dir: Vec3,
    travelled: f32,
    returning: bool,
    return_speed: f32,
    return_accel: f32,
    spin: f32,
}

impl WrenchThrow {
    /// Throws from `from` along `dir`.
    pub fn new(params: &ThrowParams, from: Vec3, dir: Vec3) -> Self {
        let dir = dir.normalize_or(Vec3::Z);
        WrenchThrow {
            params: params.clone(),
            position: from,
            velocity: dir * params.out_speed,
            dir,
            travelled: 0.0,
            returning: false,
            return_speed: 0.0,
            return_accel: 0.0,
            spin: 0.0,
        }
    }

    /// Advances one tick. `hand` is where it returns to; `blocked` means the host's sweep hit
    /// something since the last tick, which turns it around early.
    ///
    /// Out: straight along the aim at `out_speed` up to `max_distance`. Back: homing on the hand,
    /// its acceleration growing by `return_jerk` per second up to `return_max_speed`.
    /// (Assumption: the game's exact out/back curves aren't known, only these settings.)
    pub fn step(&mut self, dt: f32, hand: Vec3, blocked: bool) -> ThrowStatus {
        self.spin = (self.spin + self.params.spin_speed.to_radians() * dt) % std::f32::consts::TAU;
        if !self.returning && (blocked || self.travelled >= self.params.max_distance) {
            self.returning = true;
        }
        if !self.returning {
            let step = (self.params.out_speed * dt).min(self.params.max_distance - self.travelled);
            self.position += self.dir * step;
            self.travelled += step;
            self.velocity = self.dir * self.params.out_speed;
            return ThrowStatus::Flying;
        }
        self.return_accel += self.params.return_jerk * dt;
        self.return_speed = (self.return_speed + self.return_accel * dt).min(self.params.return_max_speed);
        let to_hand = hand - self.position;
        let dist = to_hand.length();
        let step = self.return_speed * dt;
        if dist <= self.params.catch_radius || step >= dist {
            self.position = hand;
            self.velocity = Vec3::ZERO;
            return ThrowStatus::Caught;
        }
        self.velocity = to_hand / dist * self.return_speed;
        self.position += to_hand / dist * step;
        ThrowStatus::Flying
    }

    pub fn position(&self) -> Vec3 {
        self.position
    }

    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    /// Spin angle in radians, around the axis across the throw direction.
    pub fn spin(&self) -> f32 {
        self.spin
    }

    pub fn returning(&self) -> bool {
        self.returning
    }

    /// The direction it was thrown in.
    pub fn direction(&self) -> Vec3 {
        self.dir
    }
}
