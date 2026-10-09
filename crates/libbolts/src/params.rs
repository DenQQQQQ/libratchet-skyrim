//! The numbers behind the bolts. `BoltParams::default()` holds the game's (RiftApart.exe and the
//! bolt pickups' actor data); `scaled` sizes them for a bigger or smaller hero.
//!
//! Units: metres, seconds, radians.

use crate::reward::Kind;
use std::f32::consts::FRAC_PI_4;

#[derive(Clone, Debug, PartialEq)]
pub struct BoltParams {
    /// Each pickup's PickupRange, by tier (bronze to platinum) and look: the hero, or the thrown
    /// wrench, this close sets a resting bolt flying to him.
    pub pickup_range: [[f32; 3]; 4],
    /// The burst: straight up at `burst_up`, plus `burst_speed` × (1 − `burst_falloff`·u²) (u
    /// random in 0..1) any way round, between `burst_elevation` above level and straight up.
    pub burst_up: f32,
    pub burst_speed: f32,
    pub burst_falloff: f32,
    pub burst_elevation: f32,
    /// Each bolt's own gravity, picked in this range (downward). No drag.
    pub gravity: (f32, f32),
    /// The game sweeps a sphere this big ahead of a flying bolt; a bounce leaves it this far off
    /// the wall.
    pub radius: f32,
    /// A hit counts only when the bolt moves into the surface faster than this.
    pub hit_speed: f32,
    /// Surfaces whose normal points up at least this much are floors, where a bolt lands;
    /// steeper ones are walls, which bounce it back at `bounce_keep` of its speed while it has
    /// bounced fewer than `max_bounces` times (then a wall stops it too).
    pub floor_normal_y: f32,
    pub bounce_keep: f32,
    pub max_bounces: u32,
    /// A landed bolt rests this far above the contact.
    pub rest_lift: f32,
    /// A bolt falling this fast has fallen out of the level: it's collected, credited
    /// (AutoCollectOnFallOut).
    pub fall_out_speed: f32,
    /// At rest it bobs up by `bob_height` and back, once in a period picked in `bob_period`,
    /// and spins at `spin` about a near-vertical axis.
    pub bob_height: f32,
    pub bob_period: (f32, f32),
    pub spin: f32,
    /// The pull: a speed picked in `pull_speed`, times 1 + `pull_boost` over the distances in
    /// `pull_boost_range` (from nothing at the near one to all of it at the far one, by the
    /// squared distance). It stays as picked for the whole flight.
    pub pull_speed: (f32, f32),
    pub pull_boost: f32,
    pub pull_boost_range: (f32, f32),
    /// The swarm: each bolt aims off the hero along its own direction (any way round, up to
    /// `swarm_elevation` above level), by `swarm_share` of its distance less its step, at most
    /// `swarm_max`.
    pub swarm_share: f32,
    pub swarm_max: f32,
    pub swarm_elevation: f32,
    /// Collected this close to the centre of the hero.
    pub collect_radius: f32,
    /// At most this many bolts burst out at once (the rest wait at their spawn point), and this
    /// many lie in the level (past that, the farthest one resting is collected).
    pub max_in_flight: usize,
    pub max_in_world: usize,
}

impl Default for BoltParams {
    /// The game's numbers (RiftApart.exe). The pickup ranges are the actors' PickupRange: 10 m,
    /// but 6 m for `pup_Bolt_Golden_01` (its two siblings are 10 m).
    fn default() -> Self {
        BoltParams {
            pickup_range: [[10.0; 3], [10.0; 3], [6.0, 10.0, 10.0], [10.0; 3]],
            burst_up: 2.0,
            burst_speed: 7.5,
            burst_falloff: 0.4667,
            burst_elevation: 0.5,
            gravity: (12.0, 18.75),
            radius: 0.07,
            hit_speed: 0.5,
            floor_normal_y: 0.8,
            bounce_keep: 0.5,
            max_bounces: 12,
            rest_lift: 0.15,
            fall_out_speed: 25.0,
            bob_height: 0.1,
            bob_period: (2.5, 5.0),
            // (The game's constant is π; that it's radians per second is our reading.)
            spin: std::f32::consts::PI,
            pull_speed: (9.0, 14.0),
            pull_boost: 2.0,
            pull_boost_range: (25.0, 50.0),
            swarm_share: 0.85,
            swarm_max: 7.5,
            swarm_elevation: FRAC_PI_4,
            collect_radius: 0.5,
            max_in_flight: 200,
            max_in_world: 1024,
        }
    }
}

impl BoltParams {
    /// A pickup's PickupRange.
    pub fn range(&self, kind: Kind) -> f32 {
        let (tier, look) = kind.slot();
        self.pickup_range[tier][look]
    }

    /// Distances and speeds (gravity too) times `s`, so the bolts keep their timing around a
    /// bigger or smaller hero, as libratchet's and libarsenal's scaling do.
    pub fn scaled(&self, s: f32) -> BoltParams {
        let pair = |(a, b): (f32, f32)| (a * s, b * s);
        BoltParams {
            pickup_range: self.pickup_range.map(|r| r.map(|d| d * s)),
            burst_up: self.burst_up * s,
            burst_speed: self.burst_speed * s,
            gravity: pair(self.gravity),
            radius: self.radius * s,
            hit_speed: self.hit_speed * s,
            rest_lift: self.rest_lift * s,
            fall_out_speed: self.fall_out_speed * s,
            bob_height: self.bob_height * s,
            pull_speed: pair(self.pull_speed),
            pull_boost_range: pair(self.pull_boost_range),
            swarm_max: self.swarm_max * s,
            collect_radius: self.collect_radius * s,
            ..self.clone()
        }
    }
}
