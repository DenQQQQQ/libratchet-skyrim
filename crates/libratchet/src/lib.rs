//! libratchet: Ratchet-style platformer movement as an engine-independent library.
//!
//! Like libsm64 does for Mario, this crate holds only the movement rules. A host engine feeds
//! it input and contact information every tick and moves its own character by the returned
//! velocity. Hosts so far: the Bevy sandbox (`apps/sandbox`). Planned: Skyrim via SKSE.

mod sim;
mod tuning;
mod world;
mod wrench;

pub use glam;
pub use sim::{Contact, Event, Input, LedgeMove, MoveState, Ratchet, WallContact};
pub use tuning::{
    DashParams, JumpParams, LedgeParams, LedgeWindow, MeleeParams, RecoveryParams, RootPath, SpeedParams, SwingParams, SwingshotParams, ThrowParams,
    Tuning, WallStickParams,
};
pub use world::{Ledge, LedgeFlags, NoWorld, RayHit, SwingKind, SwingTarget, World};
pub use wrench::{ThrowStatus, WrenchThrow};
