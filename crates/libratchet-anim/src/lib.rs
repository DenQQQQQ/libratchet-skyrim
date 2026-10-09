//! libratchet-anim: which of Ratchet's clips play, and how, from libratchet's state.
//!
//! A port of the Bevy sandbox's clip choice (`apps/sandbox/src/ratchet_model.rs`: `play_clips`,
//! `setup_clips`, `lean_into_turns`, `blend_swing_loop`), with the crossfades and clip clocks of
//! Bevy 0.19's animation player, so a host that samples and blends clips itself (the SKSE
//! plugin) shows what the sandbox shows. Once a frame, after that frame's sim steps, the host
//! calls [`Animator::update`] with the sim and what it knows about weapons, and gets the clips to
//! sample, each at a time and a weight. Coordinates are libratchet's: Y up, metres, yaw
//! `atan2(x, z)`.
//!
//! **Blending the layers** (what the sandbox's animation graph does with them), for each joint's
//! translation, rotation and scale on its own:
//! - Base layers ([`LayerKind::Base`], [`LayerKind::BaseUpperBody`]) first, in the order given.
//!   Among the layers whose clip has a track for it: the first one's value, then each next one
//!   moves it toward its own by its weight over the sum of the weights so far (translations and
//!   scales by lerp, rotations by slerp). The weights needn't sum to 1: the glove's hand pose
//!   weighs 20 and the wrench catch 6, so they win where they have tracks. An upper-body layer
//!   is there only for the joints in the `spine_1` branch (`spine_1` and everything under it);
//!   the others leave it out entirely.
//! - Then the additive layers, in order, on top: translation += weight × the clip's, and
//!   rotation = slerp(identity, the clip's, weight) × rotation (they have no scales). With no
//!   base layer's track to start from they start from identity and zero; what no layer has a
//!   track for keeps the value it had.
//! - Each layer is sampled at its time as is: loops come wrapped and one-shots held at their end.

mod animator;
mod clips;
mod mixer;

pub use animator::Animator;
pub use clips::clip_names;

use libratchet::glam::Vec3;

/// One of the host's clips.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipInfo {
    /// Its name as the converter's `ratchet.glb` has it ("run", "wall_stick_left",
    /// "glide_lean_additive"; see [`clip_names`]).
    pub name: String,
    /// Its length, seconds.
    pub seconds: f32,
    /// How far its root moves over it (m, at Ratchet's size), 0 if it doesn't: a walk or run
    /// that moves more than 0.5 m plays at the rate that keeps its feet planted at his speed.
    pub root_distance: f32,
    /// Seconds into it of its first JumpPeak event, if it has one: the swing jump plays so that
    /// it comes at the jump's top.
    pub jump_peak: Option<f32>,
}

impl ClipInfo {
    /// A clip with no root motion or JumpPeak event.
    pub fn new(name: impl Into<String>, seconds: f32) -> Self {
        ClipInfo { name: name.into(), seconds, root_distance: 0.0, jump_peak: None }
    }
}

/// The weapon out (the sandbox's `weapons::Slot`). The OmniWrench isn't one: it's always at
/// hand, and thrown with any weapon out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    BurstPistol,
    Shatterbomb,
}

/// What the animator needs from the host each frame besides the sim.
///
/// The counts (`shots`, `grenade_throws`, `equips`, `wrench_throws`, `wrench_catches`) are
/// running totals the host keeps, read whatever the slot: one that changed since the last
/// update starts its layer. The first update after [`Animator::new`] or [`Animator::reset`] only
/// takes them in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimInput {
    /// Seconds since the last update (at most 0.25 s counts, as with Bevy's clock).
    pub dt: f32,
    /// The size the sim's tuning is scaled to (`Tuning::scaled`, 1 = Ratchet-sized): speeds and
    /// heights are taken back to Ratchet's size to compare with the clips'. 0 or less, or not a
    /// number: the sim's own (`Tuning::scale`).
    pub scale: f32,
    /// How he really moves (m/s, sim axes, at the sim's scale): `Ratchet::velocity`, or the
    /// host's own velocity where its movement moves him instead (Skyrim's on the ground). Else
    /// the feet slide.
    pub velocity: Vec3,
    /// His feet (sim coordinates). The aim offsets bend toward `aim_target` as seen from 0.85 m
    /// (× scale) above them.
    pub feet: Vec3,
    /// Feet to ground (m, at the sim's scale), as the sim's last `Contact` had it: a swing
    /// fired less than 0.9 m (× scale) above the ground starts with the pull in.
    pub ground_distance: Option<f32>,
    /// He was put somewhere else since the last update (a respawn): no landing or let-go clip
    /// from where he was.
    pub respawned: bool,
    /// An `Event::LedgeDropped` in any sim step since the last update: the let-go clip.
    pub ledge_dropped: bool,
    /// The weapon out, or None for no weapons at all (the empty-handed clips, no aiming; the
    /// sandbox always has one).
    pub slot: Option<Slot>,
    /// The aim button is held.
    pub aiming: bool,
    /// The fire button is held.
    pub trigger: bool,
    /// Seconds left of strafing after the newest shot (above 0: still strafing).
    pub stance: f32,
    /// Gun shots so far: each one restarts the recoil layer.
    pub shots: u32,
    /// Shatterbomb throws so far: each one plays the underhand throw over his move.
    pub grenade_throws: u32,
    /// A Shatterbomb throw is winding up (the fire button held).
    pub cooking: bool,
    /// Weapons drawn so far: each one plays the draw for its grip.
    pub equips: u32,
    /// What the crosshair points at (sim coordinates).
    pub aim_target: Vec3,
    /// The wrench is out of his hand, from the throw until just after the catch: the melee grip,
    /// whatever weapon is out.
    pub wrench_throwing: bool,
    /// Wrench throws so far: each one plays the throw layer.
    pub wrench_throws: u32,
    /// Wrench catches so far: each one plays the catch.
    pub wrench_catches: u32,
}

impl Default for AnimInput {
    fn default() -> Self {
        AnimInput {
            dt: 0.0,
            scale: 0.0,
            velocity: Vec3::ZERO,
            feet: Vec3::ZERO,
            ground_distance: None,
            respawned: false,
            ledge_dropped: false,
            slot: None,
            aiming: false,
            trigger: false,
            stance: 0.0,
            shots: 0,
            grenade_throws: 0,
            cooking: false,
            equips: 0,
            aim_target: Vec3::ZERO,
            wrench_throwing: false,
            wrench_throws: 0,
            wrench_catches: 0,
        }
    }
}

/// How a layer goes into the pose (see the crate docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    /// Blended with the other base layers by weight.
    Base,
    /// A base layer for the upper body only (the `spine_1` branch): the wrench catch. A host
    /// whose skeleton has no `spine_1` leaves `throw_catch` out of its clips, as the sandbox then
    /// has no catch.
    BaseUpperBody,
    /// Added on top of the blended base layers.
    Additive,
}

/// A clip to sample this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    /// Index into the clips given to [`Animator::new`].
    pub clip: usize,
    /// Seconds into it, within its length.
    pub time: f32,
    pub weight: f32,
    pub kind: LayerKind,
}

/// The picked move clip's clock for its sounds, as the sandbox reads it: before this update
/// moves the clocks on, so the sounds come a frame after the pose, as in the sandbox. (The
/// sandbox leaves out the clips' `Play_FOL_ratchet_land`: it plays the landing sound at
/// touchdown instead, not where the jump clip ends in the air.)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundClock {
    /// Index into the clips given to [`Animator::new`].
    pub clip: usize,
    /// Its time at the last update; None when it's new since then (another clip, or the first).
    pub from: Option<f32>,
    /// Its time now (a one-shot that's over may be a little past its end).
    pub now: f32,
}

impl SoundClock {
    /// Whether a sound `t` seconds into the clip is due now: its time passed since the last
    /// update (around the end of a loop too), or, for a new clip, it's at or before its time now.
    pub fn passed(&self, t: f32) -> bool {
        match self.from {
            Some(from) if self.now >= from => t > from && t <= self.now,
            Some(from) => t > from || t <= self.now,
            None => t <= self.now,
        }
    }
}
