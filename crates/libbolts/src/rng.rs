//! A small deterministic random generator, so a seed repeats a run exactly (tests, demos).

/// xorshift32.
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        // Spread small seeds over the bits (xorshift starts slowly from a few set bits); never 0.
        let x = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
        Rng(if x == 0 { 1 } else { x })
    }

    /// Uniform in 0..1.
    pub fn unit(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in lo..hi.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// One of 0..n.
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.unit() * n as f32) as u32).min(n.saturating_sub(1))
    }
}
