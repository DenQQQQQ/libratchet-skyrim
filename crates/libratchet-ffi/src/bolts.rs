//! C API for libbolts: Rift Apart's bolts, the currency. An enemy's reward bursts out as pickups
//! that bounce, come to rest and swarm into the hero, and the HUD's counter shows what he has.
//! The header's "Bolts" part matches this file; keep the two in sync.

use crate::{RatchetVec3, rv3, sane_scale, v3};
use libbolts::glam::Vec3;
use libbolts::{BoltParams, BoltWorld, Cause, Collision, DEFAULT_LEVEL_MULTIPLIER, Hero, Hit, MAX_BOLTS, State, Tally, Tier, bolt_amount};
use std::ffi::c_void;

/// The four pickups (bronze 1 bolt, silver 3, gold 10, platinum 30).
pub const RATCHET_BOLT_BRONZE: u8 = 0;
pub const RATCHET_BOLT_SILVER: u8 = 1;
pub const RATCHET_BOLT_GOLD: u8 = 2;
pub const RATCHET_BOLT_PLATINUM: u8 = 3;

/// What a pickup is doing: waiting at its spawn point for room under the in-flight cap, bursting
/// out, resting where it landed, or swarming to the hero.
pub const RATCHET_BOLT_QUEUED: u8 = 0;
pub const RATCHET_BOLT_FLYING: u8 = 1;
pub const RATCHET_BOLT_RESTING: u8 = 2;
pub const RATCHET_BOLT_PULLED: u8 = 3;

/// How a pickup was collected: it reached the hero, fell out of the level, or was the farthest
/// resting one with too many in the level. All are credited.
pub const RATCHET_BOLT_REACHED: u8 = 0;
pub const RATCHET_BOLT_FELL_OUT: u8 = 1;
pub const RATCHET_BOLT_OVER_CAP: u8 = 2;

/// The level multiplier of the game's default economy (Megalopolis is 10 too).
pub const RATCHET_BOLTS_LEVEL_MULTIPLIER: f32 = DEFAULT_LEVEL_MULTIPLIER;
/// The most bolts the hero can carry.
pub const RATCHET_BOLTS_MAX: u32 = MAX_BOLTS;

/// A pickup to draw.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RatchetBoltPickup {
    /// Unique among a world's pickups, never reused.
    pub id: u32,
    /// `RATCHET_BOLT_BRONZE`..`RATCHET_BOLT_PLATINUM`, and which of its three looks (0..3: the
    /// game's `pup_Bolt_<tier>_01`..`_03`).
    pub tier: u8,
    pub look: u8,
    /// `RATCHET_BOLT_QUEUED`..`RATCHET_BOLT_PULLED`.
    pub state: u8,
    /// Where to draw it (its bob included), and how to turn its model (a unit quaternion x, y, z,
    /// w: its random yaw, then its spin at rest).
    pub position: RatchetVec3,
    pub rotation: [f32; 4],
}

/// A pickup collected in the last step: its bolts are the hero's.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RatchetBoltCollected {
    pub id: u32,
    pub tier: u8,
    pub look: u8,
    /// `RATCHET_BOLT_REACHED`, `RATCHET_BOLT_FELL_OUT` or `RATCHET_BOLT_OVER_CAP`.
    pub cause: u8,
    /// What it was worth.
    pub bolts: u32,
    /// Where it was.
    pub at: RatchetVec3,
}

/// Where the hero is this step.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RatchetBoltHero {
    /// His root (feet): resting bolts within their pickup range of it fly to him.
    pub position: RatchetVec3,
    /// The middle of his body: where they fly to, and are collected.
    pub center: RatchetVec3,
    /// 1 while his thrown wrench is out, at `wrench`: resting bolts within range of it fly to him
    /// too.
    pub has_wrench: u8,
    pub wrench: RatchetVec3,
}

/// The host's level as flying bolts see it: the first surface the segment from `from` to `to`
/// meets, as `RatchetWorld`'s raycast (1 with the point and its normal, or 0).
pub type RatchetBoltRaycast = Option<unsafe extern "C" fn(user: *mut c_void, from: RatchetVec3, to: RatchetVec3, point: *mut RatchetVec3, normal: *mut RatchetVec3) -> u8>;

/// Opaque handle owned by the host. Create with `ratchet_bolts_new`, free with
/// `ratchet_bolts_free`.
pub struct RatchetBolts {
    world: BoltWorld,
    seed: u32,
    /// Each world made from this handle gets the next seed.
    worlds: u32,
}

/// Opaque handle owned by the host. Create with `ratchet_tally_new`, free with
/// `ratchet_tally_free`.
pub struct RatchetTally {
    tally: Tally,
}

/// What the counter shows.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RatchetTallyView {
    /// The bolts he has.
    pub total: u32,
    /// The total on the counter, and the "+N" waiting beside it (0: none).
    pub shown: u32,
    pub gain: u32,
    /// 1 while the counter is on screen.
    pub visible: u8,
}

fn tier_code(tier: Tier) -> u8 {
    match tier {
        Tier::Bronze => RATCHET_BOLT_BRONZE,
        Tier::Silver => RATCHET_BOLT_SILVER,
        Tier::Gold => RATCHET_BOLT_GOLD,
        Tier::Platinum => RATCHET_BOLT_PLATINUM,
    }
}

fn state_code(state: State) -> u8 {
    match state {
        State::Queued => RATCHET_BOLT_QUEUED,
        State::Flying => RATCHET_BOLT_FLYING,
        State::Resting => RATCHET_BOLT_RESTING,
        State::Pulled => RATCHET_BOLT_PULLED,
    }
}

fn cause_code(cause: Cause) -> u8 {
    match cause {
        Cause::Reached => RATCHET_BOLT_REACHED,
        Cause::FellOut => RATCHET_BOLT_FELL_OUT,
        Cause::OverCap => RATCHET_BOLT_OVER_CAP,
    }
}

/// The host's raycast as libbolts asks it.
struct HostLevel {
    raycast: RatchetBoltRaycast,
    user: *mut c_void,
}

impl Collision for HostLevel {
    fn raycast(&self, from: Vec3, to: Vec3) -> Option<Hit> {
        let f = self.raycast?;
        let (mut point, mut normal) = (RatchetVec3::default(), RatchetVec3::default());
        // Safety: the host promised a valid callback for its user pointer.
        let hit = unsafe { f(self.user, rv3(from), rv3(to), &mut point, &mut normal) };
        (hit != 0).then(|| Hit { point: v3(point), normal: v3(normal).normalize_or_zero() })
    }
}

/// An empty world of bolts with the game's numbers at the hero's size `scale` (1 = Ratchet's;
/// zero, negative or not a number: 1), its randomness from `seed` (the same seed repeats a run).
#[unsafe(no_mangle)]
pub extern "C" fn ratchet_bolts_new(scale: f32, seed: u32) -> *mut RatchetBolts {
    let world = BoltWorld::new(BoltParams::default().scaled(sane_scale(scale)), seed);
    Box::into_raw(Box::new(RatchetBolts { world, seed, worlds: 1 }))
}

/// Frees a world from `ratchet_bolts_new`. Null is ignored.
///
/// # Safety
/// `bolts` must come from `ratchet_bolts_new` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_bolts_free(bolts: *mut RatchetBolts) {
    if !bolts.is_null() {
        drop(unsafe { Box::from_raw(bolts) });
    }
}

/// The numbers for another size (zero, negative or not a number: 1); the bolts stay where they
/// are.
///
/// # Safety
/// `bolts` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_bolts_set_scale(bolts: *mut RatchetBolts, scale: f32) {
    if let Some(b) = unsafe { bolts.as_mut() } {
        b.world.set_params(BoltParams::default().scaled(sane_scale(scale)));
    }
}

/// Every pickup gone, none credited (he left the place they lie in).
///
/// # Safety
/// `bolts` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_bolts_clear(bolts: *mut RatchetBolts) {
    if let Some(b) = unsafe { bolts.as_mut() } {
        let params = b.world.params().clone();
        b.world = BoltWorld::new(params, b.seed.wrapping_add(b.worlds));
        b.worlds += 1;
    }
}

/// Bolts an enemy is worth: its relative value (Rift Apart's: 1 small, 3 medium, 5 tanks, 10-20
/// large, 30 bosses) times the level's multiplier, rounded down, at least 1.
#[unsafe(no_mangle)]
pub extern "C" fn ratchet_bolt_amount(relative_value: f32, level_multiplier: f32) -> u32 {
    bolt_amount(relative_value, level_multiplier)
}

/// Drops an enemy's reward at `at` (the game's: its position 1 m up, × the hero's size, or its
/// chest): its bolts split into pickups that burst out. Returns the bolts they hold (a big reward
/// can hold a few fewer, or more). 0 with a null `bolts`.
///
/// # Safety
/// `bolts` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_bolts_drop_reward(bolts: *mut RatchetBolts, at: RatchetVec3, relative_value: f32, level_multiplier: f32) -> u32 {
    let Some(b) = (unsafe { bolts.as_mut() }) else { return 0 };
    b.world.drop_reward(v3(at), relative_value, level_multiplier)
}

/// Moves every bolt on by `dt` seconds around the hero, flying ones swept through the host's
/// `raycast` (null: nothing to hit, they fall out). What it collected is then in
/// `ratchet_bolts_collected`.
///
/// # Safety
/// `bolts` must be valid or null; `raycast` must be safe to call with `user` during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_bolts_step(bolts: *mut RatchetBolts, dt: f32, hero: RatchetBoltHero, raycast: RatchetBoltRaycast, user: *mut c_void) {
    let Some(b) = (unsafe { bolts.as_mut() }) else { return };
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let hero = Hero { position: v3(hero.position), center: v3(hero.center), wrench: (hero.has_wrench != 0).then(|| v3(hero.wrench)) };
    b.world.step(dt, &hero, &HostLevel { raycast, user });
}

/// The pickups, oldest first: writes up to `max` to `out` (may be null) and returns how many there
/// are in all.
///
/// # Safety
/// `bolts` must be valid or null; `out` must be null or have room for `max`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_bolts_pickups(bolts: *const RatchetBolts, out: *mut RatchetBoltPickup, max: u32) -> u32 {
    let Some(b) = (unsafe { bolts.as_ref() }) else { return 0 };
    let pickups = b.world.pickups();
    if !out.is_null() {
        for (i, p) in pickups.iter().take(max as usize).enumerate() {
            let q = p.rotation();
            let item = RatchetBoltPickup {
                id: p.id(),
                tier: tier_code(p.kind().tier),
                look: p.kind().variant.min(2),
                state: state_code(p.state()),
                position: rv3(p.position()),
                rotation: [q.x, q.y, q.z, q.w],
            };
            // Safety: the host gave room for `max`.
            unsafe { out.add(i).write(item) };
        }
    }
    pickups.len() as u32
}

/// The pickups the last step collected: writes up to `max` to `out` (may be null) and returns how
/// many there were.
///
/// # Safety
/// `bolts` must be valid or null; `out` must be null or have room for `max`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_bolts_collected(bolts: *const RatchetBolts, out: *mut RatchetBoltCollected, max: u32) -> u32 {
    let Some(b) = (unsafe { bolts.as_ref() }) else { return 0 };
    let collected = b.world.collected();
    if !out.is_null() {
        for (i, c) in collected.iter().take(max as usize).enumerate() {
            let item = RatchetBoltCollected {
                id: c.id,
                tier: tier_code(c.kind.tier),
                look: c.kind.variant.min(2),
                cause: cause_code(c.cause),
                bolts: c.bolts(),
                at: rv3(c.at),
            };
            // Safety: the host gave room for `max`.
            unsafe { out.add(i).write(item) };
        }
    }
    collected.len() as u32
}

/// The counter for a hero with `total` bolts (at most `RATCHET_BOLTS_MAX`), showing it, hidden.
#[unsafe(no_mangle)]
pub extern "C" fn ratchet_tally_new(total: u32) -> *mut RatchetTally {
    Box::into_raw(Box::new(RatchetTally { tally: Tally::new(total) }))
}

/// Frees a counter from `ratchet_tally_new`. Null is ignored.
///
/// # Safety
/// `tally` must come from `ratchet_tally_new` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_tally_free(tally: *mut RatchetTally) {
    if !tally.is_null() {
        drop(unsafe { Box::from_raw(tally) });
    }
}

/// Bolts collected: a gain waits as "+N" beside the old total until 2 s after the last (from 0
/// it shows at once).
///
/// # Safety
/// `tally` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_tally_add(tally: *mut RatchetTally, bolts: u32) {
    if let Some(t) = unsafe { tally.as_mut() } {
        t.tally.add(bolts);
    }
}

/// Bolts spent: shows at once.
///
/// # Safety
/// `tally` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_tally_spend(tally: *mut RatchetTally, bolts: u32) {
    if let Some(t) = unsafe { tally.as_mut() } {
        t.tally.spend(bolts);
    }
}

/// A new total (at most `RATCHET_BOLTS_MAX`), as a gain or a loss from the old.
///
/// # Safety
/// `tally` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_tally_set(tally: *mut RatchetTally, total: u32) {
    if let Some(t) = unsafe { tally.as_mut() } {
        t.tally.set(total);
    }
}

/// The counter's clocks on by `dt` seconds.
///
/// # Safety
/// `tally` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_tally_step(tally: *mut RatchetTally, dt: f32) {
    if let Some(t) = unsafe { tally.as_mut() } {
        if dt.is_finite() && dt > 0.0 {
            t.tally.step(dt);
        }
    }
}

/// What the counter shows now (all zero with a null `tally`).
///
/// # Safety
/// `tally` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_tally_view(tally: *const RatchetTally) -> RatchetTallyView {
    let Some(t) = (unsafe { tally.as_ref() }) else { return RatchetTallyView::default() };
    RatchetTallyView { total: t.tally.total(), shown: t.tally.shown(), gain: t.tally.gain(), visible: t.tally.visible() as u8 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A floor at y = 0.
    unsafe extern "C" fn floor(_user: *mut c_void, from: RatchetVec3, to: RatchetVec3, point: *mut RatchetVec3, normal: *mut RatchetVec3) -> u8 {
        if from.y < 0.0 || to.y >= 0.0 {
            return 0;
        }
        let t = from.y / (from.y - to.y);
        unsafe {
            *point = RatchetVec3 { x: from.x + (to.x - from.x) * t, y: 0.0, z: from.z + (to.z - from.z) * t };
            *normal = RatchetVec3 { x: 0.0, y: 1.0, z: 0.0 };
        }
        1
    }

    fn pickups(b: *const RatchetBolts) -> Vec<RatchetBoltPickup> {
        let n = unsafe { ratchet_bolts_pickups(b, std::ptr::null_mut(), 0) };
        let mut out = vec![RatchetBoltPickup::default(); n as usize];
        let written = unsafe { ratchet_bolts_pickups(b, out.as_mut_ptr(), n) };
        assert_eq!(written, n);
        out
    }

    /// The sandbox's dummy through the C API: 20 bronze pickups burst out, land on the floor, and
    /// swarm into a hero 4 m off once he's there; every one is credited to the counter.
    #[test]
    fn a_dummys_drop_through_the_c_api() {
        let b = ratchet_bolts_new(1.0, 0xB017);
        let dropped = unsafe { ratchet_bolts_drop_reward(b, RatchetVec3 { x: 0.0, y: 1.0, z: 0.0 }, 2.0, RATCHET_BOLTS_LEVEL_MULTIPLIER) };
        assert_eq!(dropped, 20);
        assert_eq!(ratchet_bolt_amount(2.0, RATCHET_BOLTS_LEVEL_MULTIPLIER), 20);
        let all = pickups(b);
        assert_eq!(all.len(), 20);
        assert!(all.iter().all(|p| p.tier == RATCHET_BOLT_BRONZE && p.look < 3 && p.state == RATCHET_BOLT_FLYING));
        // Far away while they land.
        let far = RatchetBoltHero { position: RatchetVec3 { x: 100.0, y: 0.0, z: 0.0 }, center: RatchetVec3 { x: 100.0, y: 0.6, z: 0.0 }, ..Default::default() };
        for _ in 0..240 {
            unsafe { ratchet_bolts_step(b, 1.0 / 120.0, far, Some(floor), std::ptr::null_mut()) };
        }
        let resting = pickups(b);
        assert!(resting.iter().all(|p| p.state == RATCHET_BOLT_RESTING && p.position.y >= 0.14 && p.position.y <= 0.26), "all resting on the floor: {resting:?}");
        for p in &resting {
            let q = p.rotation;
            assert!((q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3] - 1.0).abs() < 1e-3, "a unit rotation");
        }
        // Then he comes within 4 m: they all reach him, each credited.
        let tally = ratchet_tally_new(0);
        let near = RatchetBoltHero { position: RatchetVec3 { x: 4.0, y: 0.0, z: 0.0 }, center: RatchetVec3 { x: 4.0, y: 0.6, z: 0.0 }, ..Default::default() };
        let mut collected = 0;
        for _ in 0..600 {
            unsafe { ratchet_bolts_step(b, 1.0 / 120.0, near, Some(floor), std::ptr::null_mut()) };
            let n = unsafe { ratchet_bolts_collected(b, std::ptr::null_mut(), 0) };
            let mut got = vec![RatchetBoltCollected::default(); n as usize];
            unsafe { ratchet_bolts_collected(b, got.as_mut_ptr(), n) };
            for c in &got {
                assert_eq!((c.cause, c.bolts), (RATCHET_BOLT_REACHED, 1));
                unsafe { ratchet_tally_add(tally, c.bolts) };
                collected += c.bolts;
            }
            unsafe { ratchet_tally_step(tally, 1.0 / 120.0) };
        }
        // (The last gain waits 2 s as "+N".)
        unsafe { ratchet_tally_step(tally, 2.1) };
        assert_eq!(collected, 20);
        assert!(pickups(b).is_empty());
        let view = unsafe { ratchet_tally_view(tally) };
        assert_eq!((view.total, view.shown), (20, 20));
        unsafe {
            ratchet_tally_free(tally);
            ratchet_bolts_free(b);
        }
    }

    /// Clearing drops them all, credits none, and the next drop's ids go on.
    #[test]
    fn clearing_drops_them_uncredited() {
        let b = ratchet_bolts_new(1.0, 7);
        unsafe { ratchet_bolts_drop_reward(b, RatchetVec3 { x: 0.0, y: 1.0, z: 0.0 }, 1.0, 10.0) };
        let before = pickups(b);
        unsafe { ratchet_bolts_clear(b) };
        assert!(pickups(b).is_empty());
        assert_eq!(unsafe { ratchet_bolts_collected(b, std::ptr::null_mut(), 0) }, 0);
        unsafe { ratchet_bolts_drop_reward(b, RatchetVec3 { x: 0.0, y: 1.0, z: 0.0 }, 1.0, 10.0) };
        let after = pickups(b);
        assert_eq!(before.len(), 10);
        assert_eq!(after.len(), 10);
        unsafe { ratchet_bolts_free(b) };
    }

    /// The counter: a gain waits as "+N" for 2 s; a loss shows at once; the counter is up 3.5 s.
    #[test]
    fn the_counter_through_the_c_api() {
        let t = ratchet_tally_new(10);
        unsafe { ratchet_tally_add(t, 5) };
        let v = unsafe { ratchet_tally_view(t) };
        assert_eq!((v.total, v.shown, v.gain, v.visible), (15, 10, 5, 1));
        unsafe { ratchet_tally_step(t, 2.01) };
        let v = unsafe { ratchet_tally_view(t) };
        assert_eq!((v.shown, v.gain, v.visible), (15, 0, 1));
        unsafe { ratchet_tally_step(t, 1.5) };
        assert_eq!(unsafe { ratchet_tally_view(t) }.visible, 0);
        unsafe { ratchet_tally_spend(t, 7) };
        let v = unsafe { ratchet_tally_view(t) };
        assert_eq!((v.total, v.shown, v.gain), (8, 8, 0));
        unsafe { ratchet_tally_set(t, u32::MAX) };
        assert_eq!(unsafe { ratchet_tally_view(t) }.total, RATCHET_BOLTS_MAX);
        unsafe { ratchet_tally_free(t) };
        assert_eq!(unsafe { ratchet_tally_view(std::ptr::null()) }, RatchetTallyView::default());
    }
}
