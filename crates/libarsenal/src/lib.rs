//! libarsenal: Ratchet & Clank-style weapons as an engine-independent library, the companion to
//! libratchet's movement.
//!
//! A host engine owns aiming, collision and damage. Each tick it tells a [`Weapon`] whether the
//! trigger is down and where the muzzle and the aimed-at point are; the weapon answers with the
//! shots to spawn. The host flies each [`Projectile`], sweeps the segment it moved against its
//! world, and on impact applies the hit or the [`ExplosionParams`] falloff.
//!
//! Weapon numbers come from `WeaponParams::from_riftapart_config` (JSON the user extracts from
//! their own Rift Apart install) or generic defaults. No game data ships with this crate.

mod params;
mod projectile;
mod weapon;

pub use glam;
pub use params::{ExplosionParams, ProjectileParams, SpreadParams, Trigger, WeaponParams};
pub use projectile::{Projectile, lob_velocity};
pub use weapon::{Aim, Shot, Weapon};
