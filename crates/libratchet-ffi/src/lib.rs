//! C ABI for libratchet, so C/C++ hosts (an SKSE plugin for Skyrim) can drive the sim.
//! The matching header is `include/ratchet.h`; keep the two in sync.
//!
//! Coordinates are libratchet's: Y up, metres. Hosts with other conventions convert at the
//! boundary (Skyrim: Z up, about 70 units per metre).

use libratchet::glam::{Vec2, Vec3};
use libratchet::{Contact, Event, Input, MoveState, Ratchet, Tuning};
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

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RatchetStepResult {
    /// Move the character by `velocity * dt` this tick.
    pub velocity: RatchetVec3,
    /// 0 ground, 1 jump, 2 double jump, 3 fall, 4 glide, 5 dash, 6 air dash.
    pub state: u32,
    /// `RATCHET_EVENT_*` bits for this tick.
    pub events: u32,
    /// Facing yaw in radians, `atan2(x, z)`.
    pub facing_yaw: f32,
    /// Downward speed at landing, when `RATCHET_EVENT_LANDED` is set.
    pub landed_fall_speed: f32,
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
    let input = Input { move_dir: Vec2::new(input.move_x, input.move_z), jump: input.jump != 0, dash: input.dash != 0 };
    let contact = Contact {
        grounded: contact.grounded != 0,
        ground_distance: (contact.has_ground_distance != 0).then_some(contact.ground_distance),
        hit_ceiling: contact.hit_ceiling != 0,
    };
    let v = sim.sim.step(dt, &input, &contact, Vec3::new(position.x, position.y, position.z));
    let mut events = 0;
    let mut landed_fall_speed = 0.0;
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
        };
    }
    RatchetStepResult {
        velocity: RatchetVec3 { x: v.x, y: v.y, z: v.z },
        state: state_code(sim.sim.state()),
        events,
        facing_yaw: sim.sim.facing_yaw(),
        landed_fall_speed,
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
    fn null_is_safe() {
        unsafe {
            ratchet_free(std::ptr::null_mut());
            let r = ratchet_step(std::ptr::null_mut(), 0.01, RatchetInput::default(), RatchetContact::default(), RatchetVec3::default());
            assert_eq!(r.state, 0);
            assert_eq!(ratchet_load_configs(std::ptr::null_mut(), std::ptr::null(), 1.0), -1);
        }
    }
}
