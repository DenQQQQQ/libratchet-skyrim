//! C API for libratchet-anim: which of Ratchet's clips play, with their crossfades, clocks and
//! layers, for hosts that sample and blend his clips themselves (the SKSE plugin). The header's
//! "Animation" part matches this file; keep the two in sync.

use crate::{RATCHET_EVENT_LEDGE_DROPPED, RatchetSim, RatchetVec3, v3};
use libratchet_anim::{AnimInput, Animator, ClipInfo, LayerKind, Slot};
use std::ffi::{CStr, CString, c_char};

/// One of the host's clips (see libratchet-anim's `ClipInfo`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RatchetClipInfo {
    /// Its name (UTF-8, null-terminated) as the converter's ratchet.glb has it; copied.
    pub name: *const c_char,
    /// Its length, seconds.
    pub seconds: f32,
    /// How far its root moves over it (m, at Ratchet's size), 0 if it doesn't.
    pub root_distance: f32,
    /// Seconds into it of its first JumpPeak event, below 0 if it has none.
    pub jump_peak: f32,
}

/// The weapon out: none (no weapons at all), the Burst Pistol, or the Shatterbomb.
pub const RATCHET_SLOT_NONE: u8 = 0;
pub const RATCHET_SLOT_BURST_PISTOL: u8 = 1;
pub const RATCHET_SLOT_SHATTERBOMB: u8 = 2;

/// What the animator needs each frame besides the sim (see libratchet-anim's `AnimInput`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RatchetAnimInput {
    /// Seconds since the last update (at most 0.25 s counts).
    pub dt: f32,
    /// The sim's size (as given to `ratchet_new` or `ratchet_set_scale`); 0 or less, or not a
    /// number: the sim's own.
    pub scale: f32,
    /// How he really moves (m/s, sim axes): the step's velocity, or the host's own where its
    /// movement moves him instead.
    pub velocity: RatchetVec3,
    /// His feet (sim coordinates).
    pub feet: RatchetVec3,
    /// 1 if `ground_distance` is valid: feet to ground (m), as the last contact had it.
    pub has_ground_distance: u8,
    pub ground_distance: f32,
    /// `RATCHET_EVENT_*` of every `ratchet_step` since the last update, or'ed.
    pub events: u32,
    /// 1: he was put somewhere else since the last update (no landing from where he was).
    pub respawned: u8,
    /// `RATCHET_SLOT_*`.
    pub slot: u8,
    /// The aim button held, the fire button held (0/1).
    pub aiming: u8,
    pub trigger: u8,
    /// Seconds left of strafing after the newest shot.
    pub stance: f32,
    /// Running counts: gun shots, Shatterbomb throws.
    pub shots: u32,
    pub grenade_throws: u32,
    /// A Shatterbomb throw winding up (0/1).
    pub cooking: u8,
    /// Running count of weapons drawn.
    pub equips: u32,
    /// What the crosshair points at (sim coordinates).
    pub aim_target: RatchetVec3,
    /// The wrench is out of his hand, from the throw until just after the catch (0/1).
    pub wrench_throwing: u8,
    /// Running counts: wrench throws, catches.
    pub wrench_throws: u32,
    pub wrench_catches: u32,
}

/// How a layer goes into the pose: blended with the other base layers, the same for the upper
/// body only (the `spine_1` branch), or added on top.
pub const RATCHET_LAYER_BASE: u8 = 0;
pub const RATCHET_LAYER_BASE_UPPER_BODY: u8 = 1;
pub const RATCHET_LAYER_ADDITIVE: u8 = 2;

/// A clip to sample this frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RatchetAnimLayer {
    /// Index into the clips given to `ratchet_anim_new`.
    pub clip: u16,
    /// `RATCHET_LAYER_*`.
    pub kind: u8,
    /// Seconds into the clip, within its length.
    pub time: f32,
    pub weight: f32,
}

/// The picked move clip's clock for its sounds (see libratchet-anim's `SoundClock`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RatchetAnimClock {
    /// Index into the clips given to `ratchet_anim_new`.
    pub clip: u16,
    /// 1: it's new since the last update (another clip, or the first).
    pub restarted: u8,
    /// Its time at the last update (when `restarted` is 0), and now.
    pub previous: f32,
    pub time: f32,
}

/// Opaque handle owned by the host. Create with `ratchet_anim_new`, free with
/// `ratchet_anim_free`.
pub struct RatchetAnim {
    anim: Animator,
    /// The picked clip's name, for `ratchet_anim_clip_name`.
    name: CString,
}

fn kind_code(kind: LayerKind) -> u8 {
    match kind {
        LayerKind::Base => RATCHET_LAYER_BASE,
        LayerKind::BaseUpperBody => RATCHET_LAYER_BASE_UPPER_BODY,
        LayerKind::Additive => RATCHET_LAYER_ADDITIVE,
    }
}

/// An animator for the host's `count` clips, matched by name (the last of two with one name; a
/// clip with a null name or one that isn't UTF-8 is never matched). Null if `clips` is null with
/// a count, or there are more clips than a layer's index can say (65536).
///
/// # Safety
/// `clips` must point to `count` clip infos whose names are null or null-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_anim_new(clips: *const RatchetClipInfo, count: u32) -> *mut RatchetAnim {
    if (clips.is_null() && count > 0) || count as usize > u16::MAX as usize + 1 {
        return std::ptr::null_mut();
    }
    let infos = if count == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(clips, count as usize) } };
    let clips: Vec<ClipInfo> = infos
        .iter()
        .map(|c| {
            let name = if c.name.is_null() { "" } else { unsafe { CStr::from_ptr(c.name) }.to_str().unwrap_or("") };
            ClipInfo { name: name.to_string(), seconds: c.seconds, root_distance: c.root_distance, jump_peak: (c.jump_peak >= 0.0).then_some(c.jump_peak) }
        })
        .collect();
    Box::into_raw(Box::new(RatchetAnim { anim: Animator::new(&clips), name: CString::default() }))
}

/// Frees an animator from `ratchet_anim_new`. Null is ignored.
///
/// # Safety
/// `anim` must come from `ratchet_anim_new` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_anim_free(anim: *mut RatchetAnim) {
    if !anim.is_null() {
        drop(unsafe { Box::from_raw(anim) });
    }
}

/// Back to how `ratchet_anim_new` left it (after a load or a teleport): nothing playing, no clip
/// picked, and the next update takes the host's counts in.
///
/// # Safety
/// `anim` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_anim_reset(anim: *mut RatchetAnim) {
    if let Some(anim) = unsafe { anim.as_mut() } {
        anim.anim.reset();
        anim.name = CString::default();
    }
}

/// One frame, after that frame's `ratchet_step` calls: picks the clips and writes up to `max`
/// layers to `out` (the base layers in blend order, then the additive ones); returns how many.
/// There are never more than the clips the animator was made with. Null `anim`, `sim` or `input`
/// does nothing and returns 0; a null `out` updates and writes nothing.
///
/// # Safety
/// `anim`, `sim` and `input` must be valid or null; `out` must be null or have room for `max`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_anim_update(
    anim: *mut RatchetAnim,
    sim: *const RatchetSim,
    input: *const RatchetAnimInput,
    out: *mut RatchetAnimLayer,
    max: u32,
) -> u32 {
    let (Some(anim), Some(sim), Some(input)) = (unsafe { anim.as_mut() }, unsafe { sim.as_ref() }, unsafe { input.as_ref() }) else { return 0 };
    let input = AnimInput {
        dt: input.dt,
        scale: input.scale,
        velocity: v3(input.velocity),
        feet: v3(input.feet),
        ground_distance: (input.has_ground_distance != 0).then_some(input.ground_distance),
        respawned: input.respawned != 0,
        ledge_dropped: input.events & RATCHET_EVENT_LEDGE_DROPPED != 0,
        slot: match input.slot {
            RATCHET_SLOT_BURST_PISTOL => Some(Slot::BurstPistol),
            RATCHET_SLOT_SHATTERBOMB => Some(Slot::Shatterbomb),
            _ => None,
        },
        aiming: input.aiming != 0,
        trigger: input.trigger != 0,
        stance: input.stance,
        shots: input.shots,
        grenade_throws: input.grenade_throws,
        cooking: input.cooking != 0,
        equips: input.equips,
        aim_target: v3(input.aim_target),
        wrench_throwing: input.wrench_throwing != 0,
        wrench_throws: input.wrench_throws,
        wrench_catches: input.wrench_catches,
    };
    let layers = anim.anim.update(&sim.sim, &input);
    let n = if out.is_null() { 0 } else { layers.len().min(max as usize) };
    if n > 0 {
        let out = unsafe { std::slice::from_raw_parts_mut(out, n) };
        for (slot, l) in out.iter_mut().zip(layers) {
            *slot = RatchetAnimLayer { clip: l.clip as u16, kind: kind_code(l.kind), time: l.time, weight: l.weight };
        }
    }
    anim.name = CString::new(anim.anim.clip_name().unwrap_or("")).unwrap_or_default();
    n as u32
}

/// The move clip picked last ("swing_loop" for the Swingshot's blend of its five clips), "" before
/// the first update. Valid until the animator's next update, reset or free.
///
/// # Safety
/// `anim` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_anim_clip_name(anim: *const RatchetAnim) -> *const c_char {
    match unsafe { anim.as_ref() } {
        Some(anim) => anim.name.as_ptr(),
        None => c"".as_ptr(),
    }
}

/// The picked clip's clock from the last update, for its sounds, into `out` (may be null).
/// Returns 1, or 0 if there's no clip to sound (none picked yet; the swing loop is a blend).
///
/// # Safety
/// `anim` must be valid or null; `out` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_anim_clock(anim: *const RatchetAnim, out: *mut RatchetAnimClock) -> u8 {
    let Some(clock) = unsafe { anim.as_ref() }.and_then(|a| a.anim.sound_clock()) else { return 0 };
    if let Some(out) = unsafe { out.as_mut() } {
        *out = RatchetAnimClock { clip: clock.clip as u16, restarted: clock.from.is_none() as u8, previous: clock.from.unwrap_or(0.0), time: clock.now };
    }
    1
}

/// 1 if a sound `t` seconds into the picked clip is due this frame: its time passed since the
/// last update (around a loop's end too), or, for a clip new since then, it's at or before its
/// time now. 0 otherwise, or with no clock.
///
/// # Safety
/// `anim` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ratchet_anim_sound_due(anim: *const RatchetAnim, t: f32) -> u8 {
    unsafe { anim.as_ref() }.and_then(|a| a.anim.sound_clock()).is_some_and(|c| c.passed(t)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RatchetContact, RatchetInput, ratchet_free, ratchet_new, ratchet_step};
    use std::mem::{align_of, offset_of, size_of};

    /// Every clip the animator looks for, a second long (made up), and their names.
    fn clip_table() -> (Vec<CString>, Vec<RatchetClipInfo>) {
        let names: Vec<CString> = libratchet_anim::clip_names().into_iter().map(|n| CString::new(n).unwrap()).collect();
        let infos = names.iter().map(|n| RatchetClipInfo { name: n.as_ptr(), seconds: 1.0, root_distance: 0.0, jump_peak: -1.0 }).collect();
        (names, infos)
    }

    fn name(anim: *const RatchetAnim) -> String {
        unsafe { CStr::from_ptr(ratchet_anim_clip_name(anim)) }.to_str().unwrap().to_string()
    }

    /// The layout the header describes.
    #[test]
    fn the_structs_are_laid_out_as_the_header_says() {
        assert_eq!((size_of::<RatchetAnimLayer>(), align_of::<RatchetAnimLayer>()), (12, 4));
        assert_eq!((offset_of!(RatchetAnimLayer, kind), offset_of!(RatchetAnimLayer, time), offset_of!(RatchetAnimLayer, weight)), (2, 4, 8));
        assert_eq!(size_of::<RatchetAnimClock>(), 12);
        let p = size_of::<*const c_char>();
        assert_eq!((offset_of!(RatchetClipInfo, seconds), offset_of!(RatchetClipInfo, jump_peak)), (p, p + 8));
        // The input: C's padding after its single bytes, 92 bytes in all.
        assert_eq!((offset_of!(RatchetAnimInput, has_ground_distance), offset_of!(RatchetAnimInput, ground_distance)), (32, 36));
        assert_eq!((offset_of!(RatchetAnimInput, respawned), offset_of!(RatchetAnimInput, stance)), (44, 48));
        assert_eq!((offset_of!(RatchetAnimInput, aim_target), offset_of!(RatchetAnimInput, wrench_throwing)), (68, 80));
        assert_eq!((offset_of!(RatchetAnimInput, wrench_catches), size_of::<RatchetAnimInput>()), (88, 92));
    }

    #[test]
    fn runs_jumps_and_lands_through_the_c_api() {
        let (names, infos) = clip_table();
        let anim = unsafe { ratchet_anim_new(infos.as_ptr(), infos.len() as u32) };
        assert!(!anim.is_null());
        let sim = ratchet_new(1.0);
        let mut feet = RatchetVec3::default();
        let mut contact = RatchetContact { grounded: 1, has_ground_distance: 1, ..Default::default() };
        let mut out = [RatchetAnimLayer::default(); 8];
        let mut picks: Vec<String> = vec![];
        for frame in 0..100 {
            // Run ahead, jump at frame 30, land.
            let mut events = 0;
            let mut velocity = RatchetVec3::default();
            for tick in 0..2 {
                let input = RatchetInput { move_z: 1.0, jump: (frame == 30 && tick == 0) as u8, ..Default::default() };
                let r = unsafe { ratchet_step(sim, 1.0 / 120.0, input, contact, feet) };
                events |= r.events;
                velocity = r.velocity;
                feet.y = (feet.y + r.velocity.y / 120.0).max(0.0);
                feet.z += r.velocity.z / 120.0;
                contact = RatchetContact { grounded: (feet.y <= 0.0) as u8, has_ground_distance: 1, ground_distance: feet.y, ..Default::default() };
            }
            let input = RatchetAnimInput { dt: 1.0 / 60.0, velocity, feet, has_ground_distance: 1, ground_distance: feet.y, events, ..Default::default() };
            let n = unsafe { ratchet_anim_update(anim, sim, &input, out.as_mut_ptr(), out.len() as u32) } as usize;
            assert!(n >= 1);
            // Base layers, sharing the pose.
            let sum: f32 = out[..n].iter().map(|l| l.weight).sum();
            assert!(out[..n].iter().all(|l| l.kind == RATCHET_LAYER_BASE && (l.clip as usize) < names.len()) && (sum - 1.0).abs() < 1e-5);
            if frame == 0 {
                // The idle alone, its clock a frame in; the sound clock new at 0.
                let idle = names.iter().position(|n| n.to_str() == Ok("idle")).unwrap() as u16;
                assert_eq!(out[..n], [RatchetAnimLayer { clip: idle, kind: RATCHET_LAYER_BASE, time: 1.0 / 60.0, weight: 1.0 }]);
                let mut clock = RatchetAnimClock::default();
                assert_eq!(unsafe { ratchet_anim_clock(anim, &mut clock) }, 1);
                assert_eq!(clock, RatchetAnimClock { clip: idle, restarted: 1, previous: 0.0, time: 0.0 });
                assert!(unsafe { ratchet_anim_sound_due(anim, 0.0) } == 1 && unsafe { ratchet_anim_sound_due(anim, 0.5) } == 0);
            }
            let pick = name(anim);
            if picks.last() != Some(&pick) {
                picks.push(pick);
            }
        }
        unsafe {
            ratchet_anim_free(anim);
            ratchet_free(sim);
        }
        assert_eq!(picks, ["idle", "walk", "run", "jump", "fall", "land_run", "run"]);
    }

    #[test]
    fn null_and_odd_input_are_safe() {
        unsafe {
            assert!(ratchet_anim_new(std::ptr::null(), 3).is_null());
            ratchet_anim_free(std::ptr::null_mut());
            ratchet_anim_reset(std::ptr::null_mut());
            assert_eq!(CStr::from_ptr(ratchet_anim_clip_name(std::ptr::null())).to_bytes(), b"");
            assert_eq!(ratchet_anim_clock(std::ptr::null(), std::ptr::null_mut()), 0);
            assert_eq!(ratchet_anim_sound_due(std::ptr::null(), 0.0), 0);
            // No clips, a clip with no name: nothing plays.
            let unnamed = RatchetClipInfo { name: std::ptr::null(), seconds: f32::NAN, root_distance: -1.0, jump_peak: f32::NAN };
            for (clips, count) in [(std::ptr::null(), 0), (&unnamed as *const RatchetClipInfo, 1)] {
                let anim = ratchet_anim_new(clips, count);
                assert!(!anim.is_null());
                let sim = ratchet_new(1.0);
                let input = RatchetAnimInput { dt: 1.0 / 60.0, ..Default::default() };
                let mut out = [RatchetAnimLayer::default(); 4];
                assert_eq!(ratchet_anim_update(anim, sim, &input, out.as_mut_ptr(), 4), 0);
                assert_eq!(ratchet_anim_update(anim, std::ptr::null(), &input, out.as_mut_ptr(), 4), 0);
                assert_eq!(ratchet_anim_update(anim, sim, std::ptr::null(), out.as_mut_ptr(), 4), 0);
                ratchet_anim_free(anim);
                ratchet_free(sim);
            }
            // A null out still moves it on; a frame that makes no sense moves nothing.
            let (_names, infos) = clip_table();
            let anim = ratchet_anim_new(infos.as_ptr(), infos.len() as u32);
            let sim = ratchet_new(1.0);
            let input = RatchetAnimInput { dt: 1.0 / 60.0, ..Default::default() };
            assert_eq!(ratchet_anim_update(anim, sim, &input, std::ptr::null_mut(), 8), 0);
            assert_eq!(CStr::from_ptr(ratchet_anim_clip_name(anim)).to_bytes(), b"idle");
            let mut out = [RatchetAnimLayer::default(); 1];
            let odd = RatchetAnimInput { dt: f32::NAN, scale: f32::NAN, ..Default::default() };
            assert_eq!(ratchet_anim_update(anim, sim, &odd, out.as_mut_ptr(), 1), 1);
            assert_eq!(out[0].time, 1.0 / 60.0);
            ratchet_anim_reset(anim);
            assert_eq!(CStr::from_ptr(ratchet_anim_clip_name(anim)).to_bytes(), b"");
            ratchet_anim_free(anim);
            ratchet_free(sim);
        }
    }
}
