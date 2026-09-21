//! Deterministic, dependency-free RNG.
//!
//! `rand` would work, but the whole point of Phase 1 is reproducible
//! experiments: a given `--seed` must produce a bit-identical run so that two
//! parameter sets can be compared fairly. SplitMix64 is 3 lines, fast, and has
//! a period far larger than anything this simulation needs.

use bevy_ecs::prelude::*;

/// SplitMix64 state.
#[derive(Resource, Debug, Clone)]
pub struct SimRng {
    state: u64,
}

impl SimRng {
    /// Create a generator from a seed.
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Raw 64-bit output (SplitMix64).
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform float in `[0, 1)`.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        // 24 mantissa bits is plenty for f32 and avoids the rounding-up case
        // that a naive `as f32 / u32::MAX as f32` would introduce.
        ((self.next_u64() >> 40) as f32) * (1.0 / (1u32 << 24) as f32)
    }

    /// Uniform float in `[-1, 1)`.
    #[inline]
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// Uniform float in `[lo, hi)`.
    #[inline]
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.unit() * (hi - lo)
    }

    /// Uniform integer in `[0, n)`.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = SimRng::new(42);
        let mut b = SimRng::new(42);
        for _ in 0..1_000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seed_different_stream() {
        let mut a = SimRng::new(1);
        let mut b = SimRng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn unit_stays_in_range() {
        let mut rng = SimRng::new(7);
        for _ in 0..100_000 {
            let v = rng.unit();
            assert!((0.0..1.0).contains(&v), "unit() produced {v}");
        }
    }

    #[test]
    fn signed_is_roughly_centered() {
        let mut rng = SimRng::new(9);
        let mut sum = 0.0f64;
        const N: usize = 200_000;
        for _ in 0..N {
            let v = rng.signed();
            assert!((-1.0..1.0).contains(&v));
            sum += v as f64;
        }
        assert!((sum / N as f64).abs() < 0.01, "mean drifted: {}", sum / N as f64);
    }
}
