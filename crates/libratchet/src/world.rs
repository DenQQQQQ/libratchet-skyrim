//! What the sim asks the host's level for, beyond the contacts it reports each tick: the
//! ledges near the character.
//!
//! In Rift Apart, ledges are placed level objects (ledge paths with flags), not edges the hero
//! finds in the collision (RiftApart.exe). A host gives the sim its ledges as straight stretches;
//! one that has no designers' ledges can make them from its collision's top edges.

use glam::{Vec2, Vec3};

/// A straight stretch of ledge he can hang from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ledge {
    /// The edge, end to end.
    pub a: Vec3,
    pub b: Vec3,
    /// Out of the wall below the edge, on the ground plane (x, z), unit length.
    pub normal: Vec2,
    /// The host's id for the path it belongs to, so the sim can tell ledges apart.
    pub id: u32,
    /// What it allows (the game's per-path flags).
    pub flags: LedgeFlags,
}

/// The game's per-ledge-path switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgeFlags {
    /// Grabbing it from the air.
    pub grab: bool,
    /// Jumping up over it (the ledge jump).
    pub jump: bool,
    /// Letting go of it.
    pub drop: bool,
    /// Jumping from it to another ledge, and being jumped to.
    pub transfer: bool,
}

impl Default for LedgeFlags {
    fn default() -> Self {
        LedgeFlags { grab: true, jump: true, drop: true, transfer: true }
    }
}

impl Ledge {
    /// The point on the edge closest to `p`, and how far along it that is (0 at `a`, 1 at `b`).
    pub fn closest(&self, p: Vec3) -> (Vec3, f32) {
        let ab = self.b - self.a;
        let len2 = ab.length_squared();
        let t = if len2 > 1e-8 { ((p - self.a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
        (self.a + ab * t, t)
    }

    /// Unit direction from `a` to `b`.
    pub fn along(&self) -> Vec3 {
        (self.b - self.a).normalize_or_zero()
    }
}

/// A Swingshot target the level offers (the game's placed swingshot targets).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingTarget {
    pub position: Vec3,
    pub kind: SwingKind,
    /// The target's own forward on the ground plane: he swings in the upright plane along it.
    pub axis: Vec2,
    /// The cable's length on it (each target sets its own).
    pub length: f32,
    /// Its own range limits, where it has them.
    pub min_range: Option<f32>,
    pub max_range: Option<f32>,
    /// The host's id for it, one per target: the sim tells targets apart by it.
    pub id: u32,
}

/// What a Swingshot target does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwingKind {
    /// He swings on it.
    Swing,
    /// It reels him in (the Hookshot).
    Hook,
}

/// Where a ray hit the level: the point, and the surface's normal there (unit length).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub point: Vec3,
    pub normal: Vec3,
}

/// The host's level, as the sim queries it.
pub trait World {
    /// Adds the placed ledges with any part within `radius` of `center` to `out`.
    fn ledges(&self, center: Vec3, radius: f32, out: &mut Vec<Ledge>);
    /// The first surface a ray from `from` to `to` hits, if any (rays starting inside
    /// something needn't hit it).
    fn raycast(&self, from: Vec3, to: Vec3) -> Option<RayHit>;
    /// Adds the Swingshot targets within `radius` of `center` to `out` (none by default).
    fn swing_targets(&self, _center: Vec3, _radius: f32, _out: &mut Vec<SwingTarget>) {}
}

/// A level with nothing to ask about.
pub struct NoWorld;

impl World for NoWorld {
    fn ledges(&self, _center: Vec3, _radius: f32, _out: &mut Vec<Ledge>) {}

    fn raycast(&self, _from: Vec3, _to: Vec3) -> Option<RayHit> {
        None
    }
}
