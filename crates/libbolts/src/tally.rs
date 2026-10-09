//! The HUD's bolt counter, as the game's UI runs it (RiftApart.exe).

/// The most bolts the hero can carry.
pub const MAX_BOLTS: u32 = 9_999_999;
/// A gain shows as "+N" beside the old total until this long after the last one.
const SETTLE: f32 = 2.0;
/// The counter shows this long after any change.
const SHOW: f32 = 3.5;

/// The hero's bolts and what the counter shows of them: gains add up in a "+N" beside the old
/// total, which takes them in 2 s after the last; a loss, or a change from or to 0, shows at
/// once. The counter is up for 3.5 s after any change. (The game's count-up animation, in its UI
/// script, isn't read: the total jumps.)
#[derive(Clone, Debug)]
pub struct Tally {
    total: u32,
    shown: u32,
    /// Seconds until a waiting "+N" is taken in, and of the counter showing.
    settle: f32,
    up: f32,
}

impl Tally {
    pub fn new(total: u32) -> Self {
        let total = total.min(MAX_BOLTS);
        Tally { total, shown: total, settle: 0.0, up: 0.0 }
    }

    /// Bolts collected.
    pub fn add(&mut self, bolts: u32) {
        self.set(self.total.saturating_add(bolts));
    }

    /// Bolts spent.
    pub fn spend(&mut self, bolts: u32) {
        self.set(self.total.saturating_sub(bolts));
    }

    pub fn set(&mut self, total: u32) {
        let (old, total) = (self.total, total.min(MAX_BOLTS));
        if total == old {
            return;
        }
        self.total = total;
        self.up = SHOW;
        if total > old && old > 0 {
            self.settle = SETTLE;
        } else {
            self.shown = total;
            self.settle = 0.0;
        }
    }

    pub fn step(&mut self, dt: f32) {
        if self.settle > 0.0 {
            self.settle -= dt;
            if self.settle <= 0.0 {
                self.shown = self.total;
            }
        }
        self.up = (self.up - dt).max(0.0);
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    /// The total on the counter.
    pub fn shown(&self) -> u32 {
        self.shown
    }

    /// The "+N" beside it (0: none).
    pub fn gain(&self) -> u32 {
        self.total.saturating_sub(self.shown)
    }

    /// The counter is on screen.
    pub fn visible(&self) -> bool {
        self.up > 0.0
    }
}
