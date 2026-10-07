//! C ABI for libratchet, so C/C++ hosts (an SKSE plugin for Skyrim) can drive the sim.
//! The matching header is `include/ratchet.h`; keep the two in sync.
//!
//! Coordinates are libratchet's: Y up, metres. Hosts with other conventions convert at the
//! boundary (Skyrim: Z up, about 70 units per metre).

use libratchet::glam::{Vec2, Vec3};
use libratchet::{Contact, Event, Input, MoveState, Ratchet, ThrowStatus, Tuning, WrenchThrow};
use std::ffi::{CStr, c_char};
use std::path::Path;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RatchetVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RatchetInput {
    /// World-space direction on the ground plane, length 0..=1.
    pub move_x: f32,
    pub move_z: f32,
    /// Jump button held (0/1).
    pub jump: u8,
    /// Dash button held (0/1).
    pub dash: u8,
    /// Melee (wrench) button held (0/1).
    pub melee: u8,
    /// 1 while aiming a weapon: face (`aim_x`, `aim_z`) and strafe.
    pub aiming: u8,
    pub aim_x: f32,
    pub aim_z: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RatchetContact {
    pub grounded: u8,
    pub hit_ceiling: u8,
    /// 1 if `ground_distance` is valid.
    pub has_ground_distance: u8,
    pub ground_distance: f32,
}

pub const RATCHET_EVENT_JUMPED: u32 = 1 << 0;
pub const RATCHET_EVENT_DOUBLE_JUMPED: u32 = 1 << 1;
pub const RATCHET_EVENT_LEFT_GROUND: u32 = 1 << 2;
pub const RATCHET_EVENT_GLIDE_STARTED: u32 = 1 << 3;
pub const RATCHET_EVENT_GLIDE_ENDED: u32 = 1 << 4;
pub const RATCHET_EVENT_LANDED: u32 = 1 << 5;
pub const RATCHET_EVENT_DASHED: u32 = 1 << 6;
pub const RATCHET_EVENT_AIR_DASHED: u32 = 1 << 7;
/// A combo swing started; `melee_swing` says which.
pub const RATCHET_EVENT_MELEE_SWING: u32 = 1 << 8;
/// The swing's wrench connects now: check for hits (`melee_swing` says which swing).
pub const RATCHET_EVENT_MELEE_HIT: u32 = 1 << 9;
pub const RATCHET_EVENT_SMASH_STARTED: u32 = 1 << 10;
/// The Hyper-Strike hit the ground: apply the slam around the character.
pub const RATCHET_EVENT_SMASH_IMPACT: u32 = 1 << 11;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RatchetStepResult {
    /// Move the character by `velocity * dt` this tick.
    pub velocity: RatchetVec3,
    /// 0 ground, 1 jump, 2 double jump, 3 fall, 4 glide, 5 dash, 6 air dash, 7 melee,
    /// 8 Hyper-Strike, 9 Hyper-Strike landing.
    pub state: u32,
    /// `RATCHET_EVENT_*` bits for this tick.
    pub events: u32,
    /// Facing yaw in radians, `atan2(x, z)`.
    pub facing_yaw: f32,
    /// Downward speed at landing, when `RATCHET_EVENT_LANDED` is set.
    pub landed_fall_speed: f32,
    /// Combo swing (0 = first) in progress, or of this tick's swing/hit event.
    pub melee_swing: u32,
}

/// What a combo swing does, for hosts that apply damage.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RatchetSwingInfo {
    pub duration: f32,
    /// Seconds into the swing when it connects.
    pub hit_time: f32,
    pub damage: f32,
    /// How far it knocks a standard enemy back (metres, scaled).
    pub knockback: f32,
}

/// Opaque handle owned by the host. Create with `ratchet_new`, free with `ratchet_free`.
pub struct RatchetSim {
    sim: Ratchet,
    base: Tuning,
}

fn state_code(s: MoveState) -> u32 {
    match s {
        MoveState::Ground => 0,
        MoveState::Jump => 1,
        MoveState::DoubleJump => 2,
        MoveState::Fall => 3,
        MoveState::Glide => 4,
        MoveState::Dash => 5,
        MoveState::AirDash => 6,
        MoveState::Melee => 7,
        MoveState::Smash => 8,
        MoveState::SmashLand => 9,
    }
}

/// Creates a sim with generic default tuning, scaled by `scale` (1 = Ratchet-sized).
#[unsafe(no_mangle)]
pub extern "C" fn ratchet_new(scale: f32) -> *mut RatchetSim {
    let base = Tuning::default();
    Box::into_raw(Box::new(RatchetSim { sim: Ratchet::new(base.scaled(scale)), base }))
}

/// Frees a sim from `ratchet_new`. Null is ignored.
///
/// # Safety
/// `sim` must come from `ratchet_new` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_free(sim: *mut RatchetSim) {
    if !sim.is_null() {
        drop(unsafe { Box::from_raw(sim) });
    }
}

/// Loads tuning from extracted Rift Apart config JSON in `dir` (UTF-8 path) and applies
/// `scale`. Returns 0 on success; on failure the old tuning stays and a non-zero code returns.
///
/// # Safety
/// `sim` must be valid; `dir` must be a null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_load_configs(sim: *mut RatchetSim, dir: *const c_char, scale: f32) -> i32 {
    let (Some(sim), false) = (unsafe { sim.as_mut() }, dir.is_null()) else { return -1 };
    let Ok(dir) = unsafe { CStr::from_ptr(dir) }.to_str() else { return -2 };
    match Tuning::from_riftapart_configs(Path::new(dir)) {
        Ok(t) => {
            sim.sim.set_tuning(t.scaled(scale));
            sim.base = t;
            0
        }
        Err(_) => -3,
    }
}

/// Re-applies the loaded tuning at a new scale.
///
/// # Safety
/// `sim` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_set_scale(sim: *mut RatchetSim, scale: f32) {
    if let Some(sim) = unsafe { sim.as_mut() } {
        let t = sim.base.scaled(scale);
        sim.sim.set_tuning(t);
    }
}

/// Advances one tick. `position` is the character's feet.
///
/// # Safety
/// `sim` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_step(
    sim: *mut RatchetSim,
    dt: f32,
    input: RatchetInput,
    contact: RatchetContact,
    position: RatchetVec3,
) -> RatchetStepResult {
    let Some(sim) = (unsafe { sim.as_mut() }) else { return RatchetStepResult::default() };
    let input = Input {
        move_dir: Vec2::new(input.move_x, input.move_z),
        jump: input.jump != 0,
        dash: input.dash != 0,
        melee: input.melee != 0,
        aim: (input.aiming != 0).then(|| Vec2::new(input.aim_x, input.aim_z)),
    };
    let contact = Contact {
        grounded: contact.grounded != 0,
        ground_distance: (contact.has_ground_distance != 0).then_some(contact.ground_distance),
        hit_ceiling: contact.hit_ceiling != 0,
    };
    let v = sim.sim.step(dt, &input, &contact, Vec3::new(position.x, position.y, position.z));
    let mut events = 0;
    let mut landed_fall_speed = 0.0;
    let mut melee_swing = sim.sim.melee_swing().unwrap_or(0) as u32;
    for e in sim.sim.events() {
        events |= match e {
            Event::Jumped => RATCHET_EVENT_JUMPED,
            Event::DoubleJumped => RATCHET_EVENT_DOUBLE_JUMPED,
            Event::LeftGround => RATCHET_EVENT_LEFT_GROUND,
            Event::GlideStarted => RATCHET_EVENT_GLIDE_STARTED,
            Event::GlideEnded => RATCHET_EVENT_GLIDE_ENDED,
            Event::Dashed => RATCHET_EVENT_DASHED,
            Event::AirDashed => RATCHET_EVENT_AIR_DASHED,
            Event::Landed { fall_speed } => {
                landed_fall_speed = *fall_speed;
                RATCHET_EVENT_LANDED
            }
            Event::MeleeSwing { index } => {
                melee_swing = *index as u32;
                RATCHET_EVENT_MELEE_SWING
            }
            Event::MeleeHit { index } => {
                melee_swing = *index as u32;
                RATCHET_EVENT_MELEE_HIT
            }
            Event::SmashStarted => RATCHET_EVENT_SMASH_STARTED,
            Event::SmashImpact { .. } => RATCHET_EVENT_SMASH_IMPACT,
        };
    }
    RatchetStepResult {
        velocity: RatchetVec3 { x: v.x, y: v.y, z: v.z },
        state: state_code(sim.sim.state()),
        events,
        facing_yaw: sim.sim.facing_yaw(),
        landed_fall_speed,
        melee_swing,
    }
}

/// Fills `out` with combo swing `index` (0 = first). Returns the number of swings in the combo,
/// or 0 if `index` is out of range (`out` untouched).
///
/// # Safety
/// `sim` must be valid; `out` must be null or point to a `RatchetSwingInfo`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_swing_info(sim: *const RatchetSim, index: u32, out: *mut RatchetSwingInfo) -> u32 {
    let Some(sim) = (unsafe { sim.as_ref() }) else { return 0 };
    let combo = &sim.sim.tuning().melee.combo;
    let Some(w) = combo.get(index as usize) else { return 0 };
    if let Some(out) = unsafe { out.as_mut() } {
        *out = RatchetSwingInfo { duration: w.duration, hit_time: w.hit_time, damage: w.damage, knockback: w.knockback };
    }
    combo.len() as u32
}

/// The Hyper-Strike slam's damage and radius (metres, scaled).
///
/// # Safety
/// `sim` must be valid; `damage` and `radius` must be null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_smash_info(sim: *const RatchetSim, damage: *mut f32, radius: *mut f32) {
    let Some(sim) = (unsafe { sim.as_ref() }) else { return };
    let m = &sim.sim.tuning().melee;
    if let Some(d) = unsafe { damage.as_mut() } {
        *d = m.smash_damage;
    }
    if let Some(r) = unsafe { radius.as_mut() } {
        *r = m.smash_radius;
    }
}

/// A thrown OmniWrench. Create with `ratchet_wrench_throw`, free with `ratchet_wrench_free`.
pub struct RatchetWrench {
    throw: WrenchThrow,
}

/// Throws the wrench from `from` along `dir`, with the sim's current (scaled) throw tuning.
///
/// # Safety
/// `sim` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_wrench_throw(sim: *const RatchetSim, from: RatchetVec3, dir: RatchetVec3) -> *mut RatchetWrench {
    let Some(sim) = (unsafe { sim.as_ref() }) else { return std::ptr::null_mut() };
    let throw = WrenchThrow::new(&sim.sim.tuning().melee.throw, Vec3::new(from.x, from.y, from.z), Vec3::new(dir.x, dir.y, dir.z));
    Box::into_raw(Box::new(RatchetWrench { throw }))
}

/// Advances the thrown wrench. `blocked` = 1 if it hit something since the last tick (it turns
/// back). Writes its position and spin angle (radians). Returns 1 once it's back in the hand
/// (then free it), 0 while it flies.
///
/// # Safety
/// `wrench` must be valid; `position` and `spin` must be null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_wrench_step(
    wrench: *mut RatchetWrench,
    dt: f32,
    hand: RatchetVec3,
    blocked: u8,
    position: *mut RatchetVec3,
    spin: *mut f32,
) -> u8 {
    let Some(w) = (unsafe { wrench.as_mut() }) else { return 1 };
    let status = w.throw.step(dt, Vec3::new(hand.x, hand.y, hand.z), blocked != 0);
    let p = w.throw.position();
    if let Some(out) = unsafe { position.as_mut() } {
        *out = RatchetVec3 { x: p.x, y: p.y, z: p.z };
    }
    if let Some(out) = unsafe { spin.as_mut() } {
        *out = w.throw.spin();
    }
    (status == ThrowStatus::Caught) as u8
}

/// Frees a thrown wrench. Null is ignored.
///
/// # Safety
/// `wrench` must come from `ratchet_wrench_throw` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_wrench_free(wrench: *mut RatchetWrench) {
    if !wrench.is_null() {
        drop(unsafe { Box::from_raw(wrench) });
    }
}

/// Overrides the sim's velocity, e.g. after the host's collision stopped the character.
///
/// # Safety
/// `sim` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_set_velocity(sim: *mut RatchetSim, v: RatchetVec3) {
    if let Some(sim) = unsafe { sim.as_mut() } {
        sim.sim.set_velocity(Vec3::new(v.x, v.y, v.z));
    }
}

/// Puts the sim back on the ground with no speed (after a teleport or a load).
///
/// # Safety
/// `sim` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_reset(sim: *mut RatchetSim) {
    if let Some(sim) = unsafe { sim.as_mut() } {
        let t = sim.sim.tuning().clone();
        sim.sim = Ratchet::new(t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_through_the_c_api() {
        let sim = ratchet_new(1.0);
        let mut pos = RatchetVec3::default();
        let mut contact = RatchetContact { grounded: 1, has_ground_distance: 1, ..Default::default() };
        let mut apex: f32 = 0.0;
        let mut saw_jump = false;
        for i in 0..240 {
            let input = RatchetInput { jump: (i < 60) as u8, ..Default::default() };
            let r = unsafe { ratchet_step(sim, 1.0 / 120.0, input, contact, pos) };
            saw_jump |= r.events & RATCHET_EVENT_JUMPED != 0;
            pos.y = (pos.y + r.velocity.y / 120.0).max(0.0);
            apex = apex.max(pos.y);
            contact = RatchetContact { grounded: (pos.y <= 0.0) as u8, has_ground_distance: 1, ground_distance: pos.y, hit_ceiling: 0 };
        }
        unsafe { ratchet_free(sim) };
        assert!(saw_jump);
        let expected = Tuning::default().single_jump.height_max;
        assert!((apex - expected).abs() < 0.03, "apex {apex}");
    }

    #[test]
    fn melee_through_the_c_api() {
        let sim = ratchet_new(1.0);
        let contact = RatchetContact { grounded: 1, has_ground_distance: 1, ..Default::default() };
        let mut seen = 0;
        for i in 0..120 {
            let input = RatchetInput { melee: (i == 0) as u8, ..Default::default() };
            let r = unsafe { ratchet_step(sim, 1.0 / 120.0, input, contact, RatchetVec3::default()) };
            seen |= r.events;
            if i == 0 {
                assert_eq!(r.state, 7);
            }
        }
        let mut info = RatchetSwingInfo::default();
        let swings = unsafe { ratchet_swing_info(sim, 0, &mut info) };
        let w = unsafe { ratchet_wrench_throw(sim, RatchetVec3::default(), RatchetVec3 { x: 0.0, y: 0.0, z: 1.0 }) };
        let mut caught = 0;
        for _ in 0..1200 {
            caught = unsafe { ratchet_wrench_step(w, 1.0 / 120.0, RatchetVec3::default(), 0, std::ptr::null_mut(), std::ptr::null_mut()) };
            if caught != 0 {
                break;
            }
        }
        unsafe {
            ratchet_wrench_free(w);
            ratchet_free(sim);
        }
        assert!(seen & RATCHET_EVENT_MELEE_SWING != 0 && seen & RATCHET_EVENT_MELEE_HIT != 0);
        assert_eq!(swings, 3);
        assert!(info.hit_time > 0.0 && info.duration > info.hit_time);
        assert_eq!(caught, 1);
    }

    #[test]
    fn null_is_safe() {
        unsafe {
            ratchet_free(std::ptr::null_mut());
            let r = ratchet_step(std::ptr::null_mut(), 0.01, RatchetInput::default(), RatchetContact::default(), RatchetVec3::default());
            assert_eq!(r.state, 0);
            assert_eq!(ratchet_load_configs(std::ptr::null_mut(), std::ptr::null(), 1.0), -1);
        }
    }
}
