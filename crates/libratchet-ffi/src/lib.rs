//! C ABI for libratchet, so C/C++ hosts (an SKSE plugin for Skyrim) can drive the sim, and for
//! libratchet-anim (`anim.rs`), so they can play Ratchet's clips as the sandbox does.
//! The matching header is `include/ratchet.h`; keep the two in sync.
//!
//! Coordinates are libratchet's: Y up, metres. Hosts with other conventions convert at the
//! boundary (Skyrim: Z up, about 70 units per metre).

mod anim;
pub use anim::*;

use libratchet::glam::{Vec2, Vec3};
use libratchet::{
    Contact, Event, Input, Ledge, LedgeFlags, LedgeMove, MoveState, RayHit, Ratchet, SwingKind, SwingTarget, ThrowStatus, Tuning, WallContact, World,
    WrenchThrow,
};
use std::ffi::{CStr, c_char, c_void};
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
    /// Face (`aim_x`, `aim_z`) and strafe: 1 = aim mode (the aim button held: slower, aim-mode
    /// jumps), 2 = strafing while firing from the hip (normal speeds).
    pub aiming: u8,
    pub aim_x: f32,
    pub aim_z: f32,
    /// The camera's right on the ground plane (zero without one): wall jumps take their side
    /// from it, as in the game, and the Swingshot frames its view and pumps sideways by it
    /// (worked out from `view_forward_*` when zero).
    pub view_right_x: f32,
    pub view_right_z: f32,
    /// Crouch (the ledge drop button) held (0/1).
    pub crouch: u8,
    /// The Swingshot button held (0/1): pressed with a target in view, he swings on it (or the
    /// Hookshot reels him in) for as long as it's held.
    pub swingshot: u8,
    /// Where the camera looks (unit; zero without a camera): Swingshot targets are picked in
    /// its view, and the swing pumps along it.
    pub view_forward_x: f32,
    pub view_forward_y: f32,
    pub view_forward_z: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RatchetContact {
    pub grounded: u8,
    pub hit_ceiling: u8,
    /// 1 if `ground_distance` is valid.
    pub has_ground_distance: u8,
    pub ground_distance: f32,
    /// 1 if there's a wall beside him: the nearest within about a metre (× scale, with
    /// `wall_distance`), or one the last move ran into; while he's on a wall (state 10), that one
    /// (facing the result's `wall_normal`) when it's in reach. Its normal (out of the wall, x/z),
    /// and whether it's a wall-jump wall (marked in the level: any number of wall jumps; plain
    /// walls take one per jump).
    pub has_wall: u8,
    pub wall_sticky: u8,
    pub wall_normal_x: f32,
    pub wall_normal_z: f32,
    /// 1 inside one of the level's wall-jump zones (where the game's designers placed
    /// wall-jump stretches): wall jumps keep their line there.
    pub wall_jump_zone: u8,
    /// 1 if sweeping his flight path from here (his velocity, `gravity` from the last step
    /// down) meets a floor before a wall; only needed during wall jumps (state 11).
    pub landing_ahead: u8,
    /// 1 if `wall_distance` is valid: how far his position is from the wall's surface along its
    /// normal (m). Then he sticks from as far out as the game's probe reaches (0.775 m × scale)
    /// and is held out at its standoff (0.6 m), as in the game; without it he's pressed to a
    /// wall he touches.
    pub has_wall_distance: u8,
    pub wall_distance: f32,
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
pub const RATCHET_EVENT_WALL_STUCK: u32 = 1 << 12;
pub const RATCHET_EVENT_WALL_JUMPED: u32 = 1 << 13;
/// Grabbed a ledge; `ledge_caught` says whether it was one found on the way down.
pub const RATCHET_EVENT_LEDGE_GRABBED: u32 = 1 << 14;
pub const RATCHET_EVENT_LEDGE_CLIMBED: u32 = 1 << 15;
pub const RATCHET_EVENT_LEDGE_DROPPED: u32 = 1 << 16;
pub const RATCHET_EVENT_LEDGE_TRANSFERRED: u32 = 1 << 17;
pub const RATCHET_EVENT_LEDGE_JUMPED: u32 = 1 << 18;
/// Fired the Swingshot at a target; `swing_hook` says whether it was a Hookshot one.
pub const RATCHET_EVENT_SWING_ATTACHED: u32 = 1 << 19;
/// Let go of the swing or the hook.
pub const RATCHET_EVENT_SWING_RELEASED: u32 = 1 << 20;
pub const RATCHET_EVENT_SWING_JUMPED: u32 = 1 << 21;
/// Reeled all the way in to a Hookshot target.
pub const RATCHET_EVENT_HOOK_ARRIVED: u32 = 1 << 22;

/// A placed ledge the host offers (see libratchet's `Ledge`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RatchetLedge {
    /// The edge, end to end.
    pub a: RatchetVec3,
    pub b: RatchetVec3,
    /// Out of the wall below it, on the ground plane.
    pub normal_x: f32,
    pub normal_z: f32,
    pub id: u32,
    /// What it allows (0/1): grabbing it, the jump over it, letting go, moving to or from it.
    pub grab: u8,
    pub jump: u8,
    pub drop: u8,
    pub transfer: u8,
}

/// A Swingshot target the host offers (see libratchet's `SwingTarget`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RatchetSwingTarget {
    pub position: RatchetVec3,
    /// 0 he swings on it, 1 the Hookshot reels him in to it.
    pub kind: u8,
    /// Its own forward on the ground plane: he swings in the upright plane along it.
    pub axis_x: f32,
    pub axis_z: f32,
    /// The cable's length on it (a Hookshot target at 0 or less takes the game's).
    pub length: f32,
    /// Its own range limits; 0 or less for none.
    pub min_range: f32,
    pub max_range: f32,
    pub id: u32,
}

/// The host's level, as the sim asks about it: callbacks with the host's own pointer.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RatchetWorld {
    pub user: *mut c_void,
    /// Writes up to `max` (32) placed ledges with any part within `radius` of `center` to `out`,
    /// nearest first; returns how many. May be null (no placed ledges).
    pub ledges: Option<unsafe extern "C" fn(user: *mut c_void, center: RatchetVec3, radius: f32, out: *mut RatchetLedge, max: u32) -> u32>,
    /// The first surface the ray from `from` to `to` hits: returns 1 and fills `point` and
    /// `normal`, or 0. May be null (no ledges to catch, and the Swingshot sees every target).
    pub raycast: Option<unsafe extern "C" fn(user: *mut c_void, from: RatchetVec3, to: RatchetVec3, point: *mut RatchetVec3, normal: *mut RatchetVec3) -> u8>,
    /// Writes up to `max` (32) Swingshot targets within `radius` of `center` to `out`, nearest
    /// first; returns how many. May be null (no Swingshot targets).
    pub swing_targets: Option<unsafe extern "C" fn(user: *mut c_void, center: RatchetVec3, radius: f32, out: *mut RatchetSwingTarget, max: u32) -> u32>,
}

/// A size multiplier to use: zero, negative or not a number means Ratchet-sized.
fn sane_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
}

fn v3(v: RatchetVec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn rv3(v: Vec3) -> RatchetVec3 {
    RatchetVec3 { x: v.x, y: v.y, z: v.z }
}

impl World for RatchetWorld {
    fn ledges(&self, center: Vec3, radius: f32, out: &mut Vec<Ledge>) {
        let Some(f) = self.ledges else { return };
        let mut buf = [RatchetLedge::default(); 32];
        // Safety: the host promised a valid callback for its user pointer.
        let n = unsafe { f(self.user, rv3(center), radius, buf.as_mut_ptr(), buf.len() as u32) } as usize;
        out.extend(buf[..n.min(buf.len())].iter().map(|l| Ledge {
            a: v3(l.a),
            b: v3(l.b),
            normal: Vec2::new(l.normal_x, l.normal_z).normalize_or_zero(),
            id: l.id,
            flags: LedgeFlags { grab: l.grab != 0, jump: l.jump != 0, drop: l.drop != 0, transfer: l.transfer != 0 },
        }));
    }

    fn raycast(&self, from: Vec3, to: Vec3) -> Option<RayHit> {
        let f = self.raycast?;
        let (mut point, mut normal) = (RatchetVec3::default(), RatchetVec3::default());
        // Safety: as above.
        let hit = unsafe { f(self.user, rv3(from), rv3(to), &mut point, &mut normal) };
        (hit != 0).then(|| RayHit { point: v3(point), normal: v3(normal).normalize_or_zero() })
    }

    fn swing_targets(&self, center: Vec3, radius: f32, out: &mut Vec<SwingTarget>) {
        let Some(f) = self.swing_targets else { return };
        let mut buf = [RatchetSwingTarget::default(); 32];
        // Safety: as above.
        let n = unsafe { f(self.user, rv3(center), radius, buf.as_mut_ptr(), buf.len() as u32) } as usize;
        out.extend(buf[..n.min(buf.len())].iter().map(|t| SwingTarget {
            position: v3(t.position),
            kind: if t.kind == 1 { SwingKind::Hook } else { SwingKind::Swing },
            axis: Vec2::new(t.axis_x, t.axis_z),
            length: t.length,
            min_range: (t.min_range > 0.0).then_some(t.min_range),
            max_range: (t.max_range > 0.0).then_some(t.max_range),
            id: t.id,
        }));
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RatchetStepResult {
    /// Move the character by `velocity * dt` this tick.
    pub velocity: RatchetVec3,
    /// 0 ground, 1 jump, 2 double jump, 3 fall, 4 glide, 5 dash, 6 air dash, 7 melee,
    /// 8 Hyper-Strike, 9 Hyper-Strike landing, 10 wall stick, 11 wall jump, 12 ledge hang,
    /// 13 ledge climb, 14 ledge drop, 15 ledge jump, 16 swing, 17 Hookshot, 18 swing jump.
    pub state: u32,
    /// `RATCHET_EVENT_*` bits for this tick.
    pub events: u32,
    /// Facing yaw in radians, `atan2(x, z)`.
    pub facing_yaw: f32,
    /// Downward speed at landing, when `RATCHET_EVENT_LANDED` is set.
    pub landed_fall_speed: f32,
    /// Combo swing (0 = first) in progress, or of this tick's swing/hit event.
    pub melee_swing: u32,
    /// On a wall or in a wall jump: 1 if the game's right wall clips fit, 0 for the left.
    pub wall_right: u8,
    /// How fast he's pulled down right now (m/s²), e.g. for `RatchetContact::landing_ahead`.
    pub gravity: f32,
    /// With RATCHET_EVENT_LEDGE_GRABBED: 1 if the ledge was caught on the way down, 0 if placed.
    pub ledge_caught: u8,
    /// With RATCHET_EVENT_SWING_ATTACHED: 1 if it was a Hookshot target.
    pub swing_hook: u8,
    /// 1 if `swing_target` is set: the Swingshot target he'd fire at now, or the one he's on in
    /// states 16 and 17 (for the host's reticle and cable).
    pub has_swing_target: u8,
    pub swing_target: RatchetVec3,
    /// The clip-driven move onto a ledge he's in (state 12): 0 none (hanging), 1 grabbing a
    /// placed ledge, 2 onto a ledge he caught, 3 up to another ledge, 4 down to one.
    pub ledge_move: u8,
    /// 1 if he's on a ledge or moving to one: the point on its edge he holds, and the way the
    /// wall below it faces (x, z).
    pub has_ledge: u8,
    pub ledge_point: RatchetVec3,
    pub ledge_normal_x: f32,
    pub ledge_normal_z: f32,
    /// On a Swingshot target (states 16, 17): the swing's phase (0 swinging in, 1 past the
    /// target or, on a Hookshot target, reeled in, 2 from the first swing back), and on a swing
    /// its line (x, z, the way he came in), which the game's camera turns to look along.
    pub swing_phase: u8,
    pub swing_axis_x: f32,
    pub swing_axis_z: f32,
    /// The dash's direction (x, z) in states 5 and 6 (and after them, the last one's).
    pub dash_dir_x: f32,
    pub dash_dir_z: f32,
    /// On a swing (state 16): the game's swing-loop animation values. Lean: 0 swinging ahead, 0.5
    /// stalled at an end, 1 swinging back; Speed: toward 1 as he passes slowly under the target.
    pub swing_lean: f32,
    pub swing_speed: f32,
    /// On a wall or in a wall jump (states 10, 11): the wall's normal (x, z).
    pub wall_normal_x: f32,
    pub wall_normal_z: f32,
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
    /// From here the next swing may start and the stick may cut this one short.
    pub early_exit: f32,
    /// The swing is over here.
    pub exit: f32,
}

/// Opaque handle owned by the host. Create with `ratchet_new`, free with `ratchet_free`.
pub struct RatchetSim {
    sim: Ratchet,
    base: Tuning,
    world: Option<RatchetWorld>,
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
        MoveState::WallStick => 10,
        MoveState::WallJump => 11,
        MoveState::LedgeHang => 12,
        MoveState::LedgeClimb => 13,
        MoveState::LedgeDrop => 14,
        MoveState::LedgeJump => 15,
        MoveState::Swing => 16,
        MoveState::Hook => 17,
        MoveState::SwingJump => 18,
    }
}

/// The C API's version (`RATCHET_API_VERSION` in ratchet.h): raised with every change to its
/// structs or functions. Fields are only ever added at the end of a struct, never moved or
/// removed. 2: the animation (`ratchet_anim_*`).
pub const RATCHET_API_VERSION: u32 = 2;

/// The API version this library was built with, for a host to check against the header it was
/// built with (`RATCHET_API_VERSION`) before using anything else.
#[unsafe(no_mangle)]
pub extern "C" fn ratchet_api_version() -> u32 {
    RATCHET_API_VERSION
}

/// Creates a sim with generic default tuning, scaled by `scale` (1 = Ratchet-sized).
#[unsafe(no_mangle)]
pub extern "C" fn ratchet_new(scale: f32) -> *mut RatchetSim {
    let scale = sane_scale(scale);
    let base = Tuning::default();
    Box::into_raw(Box::new(RatchetSim { sim: Ratchet::new(base.scaled(scale)), base, world: None }))
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
    let scale = sane_scale(scale);
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
    let scale = sane_scale(scale);
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
        aim_mode: input.aiming == 1,
        crouch: input.crouch != 0,
        swingshot: input.swingshot != 0,
        view_forward: Vec3::new(input.view_forward_x, input.view_forward_y, input.view_forward_z),
        view_right: Vec2::new(input.view_right_x, input.view_right_z),
    };
    let contact = Contact {
        grounded: contact.grounded != 0,
        ground_distance: (contact.has_ground_distance != 0).then_some(contact.ground_distance),
        hit_ceiling: contact.hit_ceiling != 0,
        wall: (contact.has_wall != 0).then(|| WallContact {
            normal: Vec2::new(contact.wall_normal_x, contact.wall_normal_z).normalize_or_zero(),
            sticky: contact.wall_sticky != 0,
            distance: (contact.has_wall_distance != 0).then_some(contact.wall_distance),
        }),
        wall_jump_zone: contact.wall_jump_zone != 0,
        landing_ahead: contact.landing_ahead != 0,
    };
    let position = Vec3::new(position.x, position.y, position.z);
    let v = match sim.world {
        Some(world) => sim.sim.step_in(dt, &input, &contact, position, &world),
        None => sim.sim.step(dt, &input, &contact, position),
    };
    let mut ledge_caught = 0;
    let mut swing_hook = 0;
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
            Event::WallStuck => RATCHET_EVENT_WALL_STUCK,
            Event::WallJumped => RATCHET_EVENT_WALL_JUMPED,
            Event::LedgeGrabbed { caught } => {
                ledge_caught = *caught as u8;
                RATCHET_EVENT_LEDGE_GRABBED
            }
            Event::LedgeClimbed => RATCHET_EVENT_LEDGE_CLIMBED,
            Event::LedgeDropped => RATCHET_EVENT_LEDGE_DROPPED,
            Event::LedgeTransferred => RATCHET_EVENT_LEDGE_TRANSFERRED,
            Event::LedgeJumped => RATCHET_EVENT_LEDGE_JUMPED,
            Event::SwingAttached { hook } => {
                swing_hook = *hook as u8;
                RATCHET_EVENT_SWING_ATTACHED
            }
            Event::SwingReleased => RATCHET_EVENT_SWING_RELEASED,
            Event::SwingJumped => RATCHET_EVENT_SWING_JUMPED,
            Event::HookArrived => RATCHET_EVENT_HOOK_ARRIVED,
        };
    }
    let swing_target = sim.sim.swinging_on().map(|(t, _)| t).or(sim.sim.swing_target());
    let ledge = sim.sim.ledge();
    let swing_axis = sim.sim.swing_axis().unwrap_or(Vec3::ZERO);
    let dash_dir = sim.sim.dash_dir();
    RatchetStepResult {
        velocity: RatchetVec3 { x: v.x, y: v.y, z: v.z },
        state: state_code(sim.sim.state()),
        events,
        facing_yaw: sim.sim.facing_yaw(),
        landed_fall_speed,
        melee_swing,
        wall_right: sim.sim.wall_right() as u8,
        gravity: sim.sim.gravity(),
        ledge_caught,
        swing_hook,
        has_swing_target: swing_target.is_some() as u8,
        swing_target: rv3(swing_target.unwrap_or_default()),
        ledge_move: match sim.sim.ledge_move() {
            None => 0,
            Some(LedgeMove::Grab) => 1,
            Some(LedgeMove::FromFall) => 2,
            Some(LedgeMove::UpTo) => 3,
            Some(LedgeMove::DownTo) => 4,
        },
        has_ledge: ledge.is_some() as u8,
        ledge_point: rv3(ledge.map_or(Vec3::ZERO, |l| l.0)),
        ledge_normal_x: ledge.map_or(0.0, |l| l.1.x),
        ledge_normal_z: ledge.map_or(0.0, |l| l.1.y),
        swing_phase: sim.sim.swing_phase().unwrap_or(0),
        swing_axis_x: swing_axis.x,
        swing_axis_z: swing_axis.z,
        dash_dir_x: dash_dir.x,
        dash_dir_z: dash_dir.y,
        swing_lean: sim.sim.swing_lean_speed().map_or(0.0, |s| s.0),
        swing_speed: sim.sim.swing_lean_speed().map_or(0.0, |s| s.1),
        wall_normal_x: sim.sim.wall_normal().x,
        wall_normal_z: sim.sim.wall_normal().y,
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
        *out = RatchetSwingInfo {
            duration: w.duration,
            hit_time: w.hit_time,
            damage: w.damage,
            knockback: w.knockback,
            early_exit: w.early_exit,
            exit: w.exit,
        };
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

/// Gives the sim the host's level to ask about ledges (placed ones, and rays to find lips to
/// catch) and Swingshot targets (and rays to see them). Without it, there are neither.
///
/// # Safety
/// `sim` must be valid; the callbacks must stay valid for `world.user` while the sim steps.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_set_world(sim: *mut RatchetSim, world: RatchetWorld) {
    if let Some(sim) = unsafe { sim.as_mut() } {
        sim.world = Some(world);
    }
}

/// Puts the sim back on the ground with no speed (after a teleport or a load). Keeps its tuning
/// and world.
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

    /// The header's RATCHET_API_VERSION is the library's.
    #[test]
    fn the_header_has_the_librarys_api_version() {
        let header = include_str!("../include/ratchet.h");
        assert!(header.contains(&format!("#define RATCHET_API_VERSION {RATCHET_API_VERSION}u
")), "ratchet.h's version isn't {RATCHET_API_VERSION}");
        assert_eq!(ratchet_api_version(), RATCHET_API_VERSION);
    }

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
            contact = RatchetContact { grounded: (pos.y <= 0.0) as u8, has_ground_distance: 1, ground_distance: pos.y, ..Default::default() };
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

    /// A level for the callbacks: the floor and a 5 m block whose face looks down -X at x = 1.
    unsafe extern "C" fn block_raycast(_user: *mut c_void, from: RatchetVec3, to: RatchetVec3, point: *mut RatchetVec3, normal: *mut RatchetVec3) -> u8 {
        let boxes = [(Vec3::new(-50.0, -1.0, -50.0), Vec3::new(50.0, 0.0, 50.0)), (Vec3::new(1.0, 0.0, -3.0), Vec3::new(5.0, 5.0, 3.0))];
        let (from, d) = (v3(from), v3(to) - v3(from));
        let mut best: Option<(f32, Vec3)> = None;
        'boxes: for (lo, hi) in boxes {
            let (mut near, mut far, mut n) = (0.0f32, best.map_or(1.0, |b| b.0), Vec3::ZERO);
            for axis in 0..3 {
                if d[axis].abs() < 1e-9 {
                    if from[axis] < lo[axis] || from[axis] > hi[axis] {
                        continue 'boxes;
                    }
                    continue;
                }
                let inv = 1.0 / d[axis];
                let (enter, exit, face) = if inv > 0.0 {
                    ((lo[axis] - from[axis]) * inv, (hi[axis] - from[axis]) * inv, -Vec3::AXES[axis])
                } else {
                    ((hi[axis] - from[axis]) * inv, (lo[axis] - from[axis]) * inv, Vec3::AXES[axis])
                };
                if enter > near {
                    near = enter;
                    n = face;
                }
                far = far.min(exit);
                if near > far {
                    continue 'boxes;
                }
            }
            if n != Vec3::ZERO {
                best = Some((near, n));
            }
        }
        let Some((t, n)) = best else { return 0 };
        unsafe {
            *point = rv3(from + d * t);
            *normal = rv3(n);
        }
        1
    }

    #[test]
    fn catches_a_ledge_through_the_c_api() {
        let sim = ratchet_new(1.0);
        unsafe { ratchet_set_world(sim, RatchetWorld { user: std::ptr::null_mut(), ledges: None, raycast: Some(block_raycast), swing_targets: None }) };
        // A metre out from the block, above its top: standing first (on a floor that then goes
        // away), turned to face it, then falling.
        let mut pos = RatchetVec3 { x: 0.0, y: 5.6, z: 0.0 };
        let face = RatchetInput { move_x: 1.0, ..Default::default() };
        let mut seen = 0;
        let mut state = 0;
        let mut last = RatchetStepResult::default();
        for i in 0..280 {
            let standing = i < 40;
            let ground_distance = if standing { 0.0 } else { pos.y };
            let contact = RatchetContact { grounded: standing as u8, has_ground_distance: 1, ground_distance, ..Default::default() };
            let input = if standing { face } else { RatchetInput::default() };
            let r = unsafe { ratchet_step(sim, 1.0 / 120.0, input, contact, pos) };
            seen |= r.events;
            state = r.state;
            last = r;
            if !standing {
                pos.y += r.velocity.y / 120.0;
            }
        }
        unsafe { ratchet_free(sim) };
        assert!(seen & RATCHET_EVENT_LEDGE_GRABBED != 0, "no grab");
        assert_eq!(state, 12);
        assert!(last.has_ledge != 0 && (last.ledge_point.y - 5.0).abs() < 0.01 && last.ledge_normal_x < -0.99, "ledge {:?}", last.ledge_point);
        assert!((pos.y - (5.0 - Tuning::default().ledge.hang_down)).abs() < 0.05, "hangs at {}", pos.y);
    }

    /// The catch above, animated: the catch clip, the hang, and the let-go clip as he drops off
    /// (the step's let-go event reaching the animator in its events).
    #[test]
    fn animates_a_ledge_catch_and_its_let_go_through_the_c_api() {
        let names: Vec<std::ffi::CString> = libratchet_anim::clip_names().into_iter().map(|n| std::ffi::CString::new(n).unwrap()).collect();
        let clips: Vec<RatchetClipInfo> = names.iter().map(|n| RatchetClipInfo { name: n.as_ptr(), seconds: 1.0, root_distance: 0.0, jump_peak: -1.0 }).collect();
        let anim = unsafe { ratchet_anim_new(clips.as_ptr(), clips.len() as u32) };
        let sim = ratchet_new(1.0);
        unsafe { ratchet_set_world(sim, RatchetWorld { user: std::ptr::null_mut(), ledges: None, raycast: Some(block_raycast), swing_targets: None }) };
        let mut pos = RatchetVec3 { x: 0.0, y: 5.6, z: 0.0 };
        let face = RatchetInput { move_x: 1.0, ..Default::default() };
        let mut picks: Vec<String> = vec![];
        for frame in 0..200 {
            let (mut events, mut velocity) = (0, RatchetVec3::default());
            for tick in 0..2 {
                let i = frame * 2 + tick;
                let standing = i < 40;
                let ground_distance = if standing { 0.0 } else { pos.y };
                let contact = RatchetContact { grounded: standing as u8, has_ground_distance: 1, ground_distance, ..Default::default() };
                // Hanging a while, then the drop button.
                let input = if standing { face } else { RatchetInput { crouch: (300..304).contains(&i) as u8, ..Default::default() } };
                let r = unsafe { ratchet_step(sim, 1.0 / 120.0, input, contact, pos) };
                events |= r.events;
                velocity = r.velocity;
                if !standing {
                    pos.y += r.velocity.y / 120.0;
                }
            }
            let input = RatchetAnimInput { dt: 1.0 / 60.0, velocity, feet: pos, has_ground_distance: 1, ground_distance: pos.y, events, ..Default::default() };
            unsafe { ratchet_anim_update(anim, sim, &input, std::ptr::null_mut(), 0) };
            let pick = unsafe { CStr::from_ptr(ratchet_anim_clip_name(anim)) }.to_str().unwrap().to_string();
            if picks.last() != Some(&pick) {
                picks.push(pick);
            }
        }
        unsafe {
            ratchet_anim_free(anim);
            ratchet_free(sim);
        }
        let at = picks.iter().position(|p| p == "ledge_from_fall").unwrap_or_else(|| panic!("no catch: {picks:?}"));
        assert_eq!(picks[at..], ["ledge_from_fall", "ledge_idle", "ledge_drop", "fall"], "{picks:?}");
    }

    /// One Swingshot target, 7 m up and 4 m ahead.
    unsafe extern "C" fn one_target(_user: *mut c_void, _center: RatchetVec3, _radius: f32, out: *mut RatchetSwingTarget, max: u32) -> u32 {
        if max == 0 {
            return 0;
        }
        let position = RatchetVec3 { x: 0.0, y: 7.0, z: 4.0 };
        unsafe { *out = RatchetSwingTarget { position, kind: 0, axis_x: 0.0, axis_z: 1.0, length: 5.0, min_range: 0.0, max_range: 0.0, id: 1 } };
        1
    }

    #[test]
    fn swings_through_the_c_api() {
        let sim = ratchet_new(1.0);
        unsafe { ratchet_set_world(sim, RatchetWorld { user: std::ptr::null_mut(), ledges: None, raycast: None, swing_targets: Some(one_target) }) };
        // The camera behind him, looking ahead (+Z) and a little up.
        let view = Vec3::new(0.0, 0.3, 1.0).normalize();
        let mut pos = RatchetVec3::default();
        let (mut seen, mut state, mut target, mut last) = (0, 0, None, RatchetStepResult::default());
        for i in 0..120 {
            let contact = RatchetContact { grounded: (pos.y <= 0.0) as u8, has_ground_distance: 1, ground_distance: pos.y, ..Default::default() };
            let input = RatchetInput {
                swingshot: (i > 0) as u8,
                view_forward_x: view.x,
                view_forward_y: view.y,
                view_forward_z: view.z,
                view_right_x: -1.0,
                ..Default::default()
            };
            let r = unsafe { ratchet_step(sim, 1.0 / 120.0, input, contact, pos) };
            seen |= r.events;
            state = r.state;
            last = r;
            if r.has_swing_target != 0 {
                target = Some(r.swing_target);
            }
            pos.y = (pos.y + r.velocity.y / 120.0).max(0.0);
            pos.z += r.velocity.z / 120.0;
        }
        unsafe { ratchet_free(sim) };
        assert!(seen & RATCHET_EVENT_SWING_ATTACHED != 0, "never fired");
        assert_eq!(state, 16);
        assert!((last.swing_axis_z - 1.0).abs() < 1e-4 && last.swing_axis_x.abs() < 1e-4, "swing line ({}, {})", last.swing_axis_x, last.swing_axis_z);
        assert_eq!(target, Some(RatchetVec3 { x: 0.0, y: 7.0, z: 4.0 }));
    }

    #[test]
    fn null_is_safe() {
        unsafe {
            ratchet_free(std::ptr::null_mut());
            let r = ratchet_step(std::ptr::null_mut(), 0.01, RatchetInput::default(), RatchetContact::default(), RatchetVec3::default());
            assert_eq!(r.state, 0);
            assert_eq!(ratchet_load_configs(std::ptr::null_mut(), std::ptr::null(), 1.0), -1);
            // A scale that makes no sense is Ratchet-sized.
            for scale in [0.0, -2.0, f32::NAN, f32::INFINITY] {
                let sim = ratchet_new(scale);
                let r = ratchet_step(sim, 0.01, RatchetInput { jump: 1, ..Default::default() }, RatchetContact { grounded: 1, has_ground_distance: 1, ..Default::default() }, RatchetVec3::default());
                assert!(r.velocity.y.is_finite() && r.velocity.y > 1.0, "scale {scale}: jumped at {}", r.velocity.y);
                ratchet_free(sim);
            }
        }
    }
}
