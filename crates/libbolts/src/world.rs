//! The bolts lying about the level and flying through it: bursts, bounces, rest, and the swarm
//! to the hero (RiftApart.exe).

use crate::params::BoltParams;
use crate::reward::{Kind, bolt_amount, pickup_budget, split};
use crate::rng::Rng;
use glam::{Quat, Vec3};
use std::f32::consts::PI;

/// Where a ray met the level: the point, and the surface's normal there (unit length).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub point: Vec3,
    pub normal: Vec3,
}

/// The host's level, as flying bolts see it.
pub trait Collision {
    /// The first surface the segment from `from` to `to` meets, if any.
    fn raycast(&self, from: Vec3, to: Vec3) -> Option<Hit>;
}

impl<F: Fn(Vec3, Vec3) -> Option<Hit>> Collision for F {
    fn raycast(&self, from: Vec3, to: Vec3) -> Option<Hit> {
        self(from, to)
    }
}

/// Where the hero is this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hero {
    /// His root (feet): resting bolts within their PickupRange of it fly to him.
    pub position: Vec3,
    /// The centre of his body (the game takes his bounding sphere's): where they fly to, and
    /// are collected.
    pub center: Vec3,
    /// The thrown wrench, while one is out: resting bolts within range of it fly to him too.
    pub wrench: Option<Vec3>,
}

/// What a pickup is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Waiting at its spawn point for room under the in-flight cap. It can be pulled from here.
    Queued,
    /// Bursting out: flying, bouncing off walls.
    Flying,
    /// Landed, bobbing and spinning where it lies; it never goes away by itself.
    Resting,
    /// Swarming to the hero.
    Pulled,
}

/// How a pickup was collected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// It reached the hero.
    Reached,
    /// It fell out of the level.
    FellOut,
    /// Too many lay in the level, and it was the farthest from the hero.
    OverCap,
}

/// A pickup collected this tick: its bolts are the hero's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Collected {
    pub id: u32,
    pub kind: Kind,
    pub at: Vec3,
    pub cause: Cause,
}

impl Collected {
    pub fn bolts(&self) -> u32 {
        self.kind.count()
    }
}

/// A bolt pickup in the level.
#[derive(Clone, Debug)]
pub struct Pickup {
    id: u32,
    kind: Kind,
    state: State,
    /// Where it is (at rest, without the bob).
    position: Vec3,
    velocity: Vec3,
    gravity: f32,
    bounces: u32,
    /// Its random turn about the vertical, and its spin at rest.
    yaw: f32,
    spin_axis: Vec3,
    spin: f32,
    /// Seconds into its bob, the bob's period, and how far up the bob has it.
    bob: f32,
    bob_period: f32,
    lift: f32,
    /// The pull's speed, the bolt's own swarm direction, and where the hero's centre was last
    /// tick (each bolt keeps its own, from when its pull starts: ours).
    pull_speed: f32,
    swarm: Vec3,
    target: Vec3,
}

/// How far up the bob is (0..1 of its height) `phase` periods in: the game's 16 steps of
/// (1 − cos(i·π/8))·0.05 m, up to 0.1 m (RiftApart.exe), read in between (ours).
fn bob_share(phase: f32) -> f32 {
    let step = |i: usize| (1.0 - (i as f32 * PI / 8.0).cos()) * 0.5;
    let at = phase.fract() * 16.0;
    let (i, t) = (at as usize % 16, at.fract());
    step(i) + (step((i + 1) % 16) - step(i)) * t
}

impl Pickup {
    /// The host's handle on it, unique in its world.
    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Where to draw it, bob included.
    pub fn position(&self) -> Vec3 {
        self.position + Vec3::Y * self.lift
    }

    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    /// Its own gravity (downward).
    pub fn gravity(&self) -> f32 {
        self.gravity
    }

    /// Walls it has bounced off.
    pub fn bounces(&self) -> u32 {
        self.bounces
    }

    /// How to turn its model: its random yaw, then its spin.
    pub fn rotation(&self) -> Quat {
        Quat::from_axis_angle(self.spin_axis, self.spin) * Quat::from_rotation_y(self.yaw)
    }
}

/// The bolts in a level.
#[derive(Clone, Debug)]
pub struct BoltWorld {
    params: BoltParams,
    rng: Rng,
    pickups: Vec<Pickup>,
    next_id: u32,
    collected: Vec<Collected>,
}

impl BoltWorld {
    pub fn new(params: BoltParams, seed: u32) -> Self {
        BoltWorld { params, rng: Rng::new(seed), pickups: vec![], next_id: 1, collected: vec![] }
    }

    pub fn params(&self) -> &BoltParams {
        &self.params
    }

    /// New numbers (a size change); the bolts stay where they are.
    pub fn set_params(&mut self, params: BoltParams) {
        self.params = params;
    }

    /// The pickups, oldest first.
    pub fn pickups(&self) -> &[Pickup] {
        &self.pickups
    }

    /// The pickups collected in the last step.
    pub fn collected(&self) -> &[Collected] {
        &self.collected
    }

    /// Drops an enemy's reward at `at` (the game uses the enemy's position 1 m up, or its
    /// SpawnLoc locator, usually its chest): its bolts split into pickups that burst out.
    /// Returns the bolts the pickups hold (a big reward can hold fewer, see `split`).
    pub fn drop_reward(&mut self, at: Vec3, relative_value: f32, level_multiplier: f32) -> u32 {
        let kinds = split(bolt_amount(relative_value, level_multiplier), pickup_budget(relative_value, level_multiplier), &mut self.rng);
        self.spawn(at, &kinds);
        kinds.iter().map(|k| k.count()).sum()
    }

    /// Bursts these pickups out of `at`; past the in-flight cap they wait there, behind any
    /// already waiting.
    pub fn spawn(&mut self, at: Vec3, kinds: &[Kind]) {
        let waiting = self.pickups.iter().any(|p| p.state == State::Queued);
        let mut room = if waiting { 0 } else { self.params.max_in_flight.saturating_sub(self.count(State::Flying)) };
        for &kind in kinds {
            let state = if room > 0 { State::Flying } else { State::Queued };
            room = room.saturating_sub(1);
            let pickup = self.burst(at, kind, state);
            self.pickups.push(pickup);
        }
    }

    /// A new pickup with its launch, look and bob drawn (RiftApart.exe): straight up at 2 m/s
    /// plus 7.5 × (1 − 0.4667·u²) m/s any way round, 0.5 rad above level to straight up; its
    /// own gravity, 12 to 18.75 m/s².
    fn burst(&mut self, at: Vec3, kind: Kind, state: State) -> Pickup {
        let p = &self.params;
        let rng = &mut self.rng;
        let u = rng.unit();
        let speed = p.burst_speed * (1.0 - p.burst_falloff * u * u);
        let yaw = rng.range(-PI, PI);
        let elevation = rng.range(p.burst_elevation, PI / 2.0);
        let out = Vec3::new(yaw.sin() * speed * elevation.cos(), (elevation.sin() * speed).abs(), yaw.cos() * speed * elevation.cos());
        let gravity = rng.range(p.gravity.0, p.gravity.1);
        // The spin axis, tilted off vertical by up to half a unit each way (the axis's shape is
        // the game's; that each tilt is its own draw in -1..1 is ours).
        let spin_axis = Vec3::new(rng.range(-1.0, 1.0) * 0.5, 1.0, rng.range(-1.0, 1.0) * 0.5).normalize();
        let id = self.next_id;
        self.next_id += 1;
        Pickup {
            id,
            kind,
            state,
            position: at,
            velocity: Vec3::Y * p.burst_up + out,
            gravity,
            bounces: 0,
            yaw: rng.range(-PI, PI),
            spin_axis,
            spin: 0.0,
            bob: 0.0,
            bob_period: rng.range(p.bob_period.0, p.bob_period.1),
            lift: 0.0,
            pull_speed: 0.0,
            swarm: Vec3::ZERO,
            target: Vec3::ZERO,
        }
    }

    fn count(&self, state: State) -> usize {
        self.pickups.iter().filter(|p| p.state == state).count()
    }

    /// Advances every bolt by `dt` around `hero`, sweeping flying ones against `level`.
    pub fn step(&mut self, dt: f32, hero: &Hero, level: &impl Collision) {
        self.collected.clear();
        // Resting or queued bolts in range start for him; a bolt in flight never does.
        for i in 0..self.pickups.len() {
            let p = &self.pickups[i];
            if !matches!(p.state, State::Resting | State::Queued) {
                continue;
            }
            let range = self.params.range(p.kind);
            let at = p.position();
            if at.distance(hero.position) < range || hero.wrench.is_some_and(|w| at.distance(w) < range) {
                self.start_pull(i, hero.center);
            }
        }
        // Queued bolts launch as slots free, in the order they came.
        let mut room = self.params.max_in_flight.saturating_sub(self.count(State::Flying));
        for p in self.pickups.iter_mut().filter(|p| p.state == State::Queued) {
            if room == 0 {
                break;
            }
            p.state = State::Flying;
            room -= 1;
        }
        let (params, collected) = (&self.params, &mut self.collected);
        self.pickups.retain_mut(|p| {
            let cause = match p.state {
                State::Queued => None,
                State::Flying => fly(p, dt, params, level),
                State::Resting => {
                    p.bob += dt;
                    p.lift = params.bob_height * bob_share(p.bob / p.bob_period);
                    p.spin = (p.spin + params.spin * dt) % std::f32::consts::TAU;
                    None
                }
                State::Pulled => swarm(p, dt, params, hero.center),
            };
            if let Some(cause) = cause {
                collected.push(Collected { id: p.id, kind: p.kind, at: p.position, cause });
            }
            cause.is_none()
        });
        // Too many in the level: the resting one farthest from him is collected, one a tick (the
        // game takes one a frame).
        if self.pickups.len() > self.params.max_in_world
            && let Some(i) = (0..self.pickups.len())
                .filter(|&i| self.pickups[i].state == State::Resting)
                .max_by(|&a, &b| self.pickups[a].position.distance_squared(hero.position).total_cmp(&self.pickups[b].position.distance_squared(hero.position)))
        {
            let p = self.pickups.remove(i);
            self.collected.push(Collected { id: p.id, kind: p.kind, at: p.position, cause: Cause::OverCap });
        }
    }

    /// Sets bolt `i` flying to the hero: its speed (9 to 14 m/s, up to 3 times that from 25 m
    /// to 50 m away) and its swarm direction (RiftApart.exe).
    fn start_pull(&mut self, i: usize, target: Vec3) {
        let p = &self.params;
        let from = self.pickups[i].position();
        let (near, far) = p.pull_boost_range;
        let boost = ((from.distance_squared(target) - near * near) / (far * far - near * near).max(1e-6)).clamp(0.0, 1.0);
        let speed = self.rng.range(p.pull_speed.0, p.pull_speed.1) * (1.0 + p.pull_boost * boost);
        let yaw = self.rng.range(-PI, PI);
        let elevation = self.rng.range(0.0, p.swarm_elevation);
        let swarm = Vec3::new(yaw.sin() * elevation.cos(), elevation.sin(), yaw.cos() * elevation.cos());
        let b = &mut self.pickups[i];
        b.position = from;
        b.lift = 0.0;
        b.velocity = Vec3::ZERO;
        b.pull_speed = speed;
        b.swarm = swarm;
        b.target = target;
        b.state = State::Pulled;
    }
}

/// One tick of a burst bolt's flight: exact under its gravity, the segment swept against the
/// level. The game sweeps a 0.07 m sphere ahead of it; a ray along each tick's move is close
/// enough at sandbox tick rates (ours). Returns how it was collected, if it fell out.
fn fly(p: &mut Pickup, dt: f32, params: &BoltParams, level: &impl Collision) -> Option<Cause> {
    let g = Vec3::NEG_Y * p.gravity;
    let next = p.position + p.velocity * dt + 0.5 * g * dt * dt;
    let velocity = p.velocity + g * dt;
    match level.raycast(p.position, next) {
        Some(hit) if hit.normal.dot(velocity) < -params.hit_speed => {
            if hit.normal.y < params.floor_normal_y && p.bounces < params.max_bounces {
                p.velocity = (velocity - 2.0 * velocity.dot(hit.normal) * hit.normal) * params.bounce_keep;
                p.position = hit.point + hit.normal * params.radius;
                p.bounces += 1;
            } else {
                p.position = hit.point + Vec3::Y * params.rest_lift;
                p.velocity = Vec3::ZERO;
                p.state = State::Resting;
            }
            None
        }
        _ => {
            p.position = next;
            p.velocity = velocity;
            (velocity.y <= -params.fall_out_speed).then_some(Cause::FellOut)
        }
    }
}

/// One tick of the swarm (RiftApart.exe): the step is the bolt's speed plus how far the hero
/// moved along the way to him, toward a point off him along the bolt's own direction by 0.85 of
/// the distance less the step, at most 7.5 m, so the bolts close in from all round. Returns
/// `Reached` once it's within 0.5 m of his centre.
fn swarm(p: &mut Pickup, dt: f32, params: &BoltParams, target: Vec3) -> Option<Cause> {
    let to = target - p.position;
    let dist = to.length();
    let dir = to.normalize_or_zero();
    let step = dir.dot(target - p.target).abs() + p.pull_speed * dt;
    p.target = target;
    let off = (params.swarm_share * dist - step).clamp(0.0, params.swarm_max);
    let aim = target + p.swarm * off;
    let way = aim - p.position;
    p.position += way.normalize_or_zero() * step.min(way.length());
    (p.position.distance(target) < params.collect_radius).then_some(Cause::Reached)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reward::Tier;

    /// A wall at x = 1 facing -X, and a bolt flying at it with `bounces` behind it.
    fn at_the_wall(bounces: u32) -> (BoltWorld, impl Collision) {
        let wall = |from: Vec3, to: Vec3| {
            (from.x < 1.0 && to.x >= 1.0).then(|| Hit { point: from.lerp(to, (1.0 - from.x) / (to.x - from.x)), normal: Vec3::NEG_X })
        };
        let mut w = BoltWorld::new(BoltParams::default(), 1);
        w.spawn(Vec3::new(0.95, 5.0, 0.0), &[Kind { tier: Tier::Bronze, variant: 0 }]);
        let p = &mut w.pickups[0];
        p.velocity = Vec3::new(8.0, 0.0, 0.0);
        p.bounces = bounces;
        (w, wall)
    }

    fn far_hero() -> Hero {
        Hero { position: Vec3::splat(1000.0), center: Vec3::splat(1000.0), wrench: None }
    }

    #[test]
    fn walls_bounce_twelve_times_then_stop_it() {
        let (mut w, wall) = at_the_wall(11);
        w.step(1.0 / 60.0, &far_hero(), &wall);
        let p = &w.pickups[0];
        assert_eq!((p.state, p.bounces), (State::Flying, 12));
        assert!(p.velocity.x < 0.0 && (p.velocity.x + 4.0).abs() < 1e-3, "bounced back at half speed: {}", p.velocity);

        let (mut w, wall) = at_the_wall(12);
        w.step(1.0 / 60.0, &far_hero(), &wall);
        let p = &w.pickups[0];
        assert_eq!(p.state, State::Resting);
        assert!((p.position.x - 1.0).abs() < 1e-4, "rests at the contact: {}", p.position);
    }

    #[test]
    fn the_bob_follows_the_games_table() {
        let table = |i: f32| (1.0 - (i * PI / 8.0).cos()) * 0.05;
        for (phase, want) in [(0.0, 0.0), (0.25, 0.05), (0.5, 0.1), (0.75, 0.05), (15.0 / 16.0, table(15.0)), (1.0, 0.0), (1.0 / 32.0, table(1.0) * 0.5)] {
            let got = 0.1 * bob_share(phase);
            assert!((got - want).abs() < 1e-5, "{phase} of the way: {got} m, not {want}");
        }
    }
}
