//! libratchet: Ratchet-style platformer movement as an engine-independent library.
//!
//! Like libsm64 does for Mario, this crate holds only the movement rules. A host engine feeds
//! it input and contact information every tick and moves its own character by the returned
//! velocity. Hosts so far: the Bevy sandbox (`apps/sandbox`). Planned: Skyrim via SKSE.

mod sim;
mod tuning;
mod wrench;

pub use glam;
pub use sim::{Contact, Event, Input, MoveState, Ratchet};
pub use tuning::{DashParams, JumpParams, MeleeParams, SpeedParams, SwingParams, ThrowParams, Tuning};
pub use wrench::{ThrowStatus, WrenchThrow};
