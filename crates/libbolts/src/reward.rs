//! What an enemy is worth, and how the game splits it into pickups (RiftApart.exe).

use crate::rng::Rng;

/// The level's bolt multiplier when it has no economy config of its own: `system_econ_default`'s
/// BoltMultiplier (RiftApart.exe; Megalopolis is 10 too, Sargasso 12.485, the finale 81.66).
pub const DEFAULT_LEVEL_MULTIPLIER: f32 = 10.0;

/// The four bolt pickups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    Bronze,
    Silver,
    Gold,
    Platinum,
}

impl Tier {
    pub const ALL: [Tier; 4] = [Tier::Bronze, Tier::Silver, Tier::Gold, Tier::Platinum];

    /// Bolts one pickup is worth (the pickup's Count, RiftApart.exe).
    pub fn count(self) -> u32 {
        match self {
            Tier::Bronze => 1,
            Tier::Silver => 3,
            Tier::Gold => 10,
            Tier::Platinum => 30,
        }
    }

    /// As the game's actors spell it (`pup_Bolt_Golden_01`).
    pub fn name(self) -> &'static str {
        match self {
            Tier::Bronze => "bronze",
            Tier::Silver => "silver",
            Tier::Gold => "golden",
            Tier::Platinum => "platinum",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// One of the game's twelve bolt pickups: a tier, and which of its three looks (0..3, the
/// actors' `_01` to `_03`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kind {
    pub tier: Tier,
    pub variant: u8,
}

impl Kind {
    pub fn count(self) -> u32 {
        self.tier.count()
    }

    /// Where it sits in tables by tier and look.
    pub(crate) fn slot(self) -> (usize, usize) {
        (self.tier.index(), self.variant.min(2) as usize)
    }
}

/// Bolts an enemy is worth: its RelativeValue times the level's BoltMultiplier, rounded down, at
/// least 1 (RiftApart.exe). The game also multiplies by the killing weapon's kBolts upgrade and
/// by armor perks (Carbonox Advanced); both are 1 without them, and difficulty never enters.
pub fn bolt_amount(relative_value: f32, level_multiplier: f32) -> u32 {
    (relative_value * level_multiplier).max(1.0) as u32
}

/// The most pickups a drop is split into (RiftApart.exe): 15 for the smallest enemies at a
/// multiplier of 10, up to twice that for RelativeValue 10 and up, and twice again as the
/// multiplier goes from 10 to 100.
pub fn pickup_budget(relative_value: f32, level_multiplier: f32) -> u32 {
    let value = ((relative_value - 1.0) / 9.0).clamp(0.0, 1.0).sqrt() + 1.0;
    let level = ((level_multiplier - 10.0) / 90.0).clamp(0.0, 1.0) + 1.0;
    (value * level * 15.0 + 0.5) as u32
}

/// Splits `bolts` into at most `budget` pickups, as the game does (RiftApart.exe): while bolts
/// and budget are left, the smallest pickup worth at least the bolts left over the budget left
/// (one of its three looks at random), or, when none is, the first platinum one. So a big
/// reward can run out of budget and lose bolts, and the last pickups can pay a few too many.
pub fn split(bolts: u32, budget: u32, rng: &mut Rng) -> Vec<Kind> {
    let (mut left, mut budget) = (bolts as i64, budget);
    let mut out = Vec::with_capacity(budget as usize);
    while left >= 1 && budget >= 1 {
        let share = left as f32 / budget as f32;
        let kind = match Tier::ALL.into_iter().find(|t| t.count() as f32 >= share) {
            Some(tier) => Kind { tier, variant: rng.below(3) as u8 },
            None => Kind { tier: Tier::Platinum, variant: 0 },
        };
        left -= kind.count() as i64;
        budget -= 1;
        out.push(kind);
    }
    out
}
