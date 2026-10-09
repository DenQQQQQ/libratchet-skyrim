//! libbolts: Ratchet & Clank: Rift Apart's bolts (the currency) as an engine-independent
//! library, beside libratchet's movement and libarsenal's weapons. The rules are the game's,
//! read from RiftApart.exe and its pickups' data.
//!
//! When an enemy goes down, the host calls [`BoltWorld::drop_reward`] with the enemy's worth
//! (its RelativeValue: 1 small, 3 medium, 5 tanks; the game's test dummy is 2) and the level's
//! multiplier ([`DEFAULT_LEVEL_MULTIPLIER`]). The bolts are split into pickups ([`split`]) that
//! burst out, bounce off walls and come to rest; once the hero comes within a pickup's range
//! (or his thrown wrench does), it swarms to him and is collected.
//!
//! The host owns the rest:
//! - **Collision:** each tick, [`BoltWorld::step`] sweeps flying bolts through the host's
//!   [`Collision`] (one segment query, returning the point and the surface's normal).
//! - **Rendering:** it draws each of [`BoltWorld::pickups`] by its [`Kind`] (the game's twelve
//!   models: four tiers, three looks each), at its position and rotation, and removes those
//!   [`BoltWorld::collected`] reports.
//! - **Sound:** the game plays its collect sound for each pickup collected, and none for a
//!   drop or a bounce.
//! - **The HUD:** it adds each collected pickup's bolts to a [`Tally`] and shows that.
//!
//! [`BoltParams::scaled`] sizes everything for a bigger hero (Skyrim's Dragonborn). No game data
//! ships with this crate.

mod params;
mod reward;
mod rng;
mod tally;
mod world;

pub use glam;
pub use params::BoltParams;
pub use reward::{DEFAULT_LEVEL_MULTIPLIER, Kind, Tier, bolt_amount, pickup_budget, split};
pub use rng::Rng;
pub use tally::{MAX_BOLTS, Tally};
pub use world::{BoltWorld, Cause, Collected, Collision, Hero, Hit, Pickup, State};
