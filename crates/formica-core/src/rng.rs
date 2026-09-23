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

    /// Standard normal, via the Box-Muller transform.
    ///
    /// Needed for the per-ant behaviour traits: those are drawn log-normally so
    /// that most ants are near the population mean while a thin tail of
    /// unusually bold or unusually timid individuals exists. A uniform draw
    /// cannot produce that tail, and the tail is the whole point - it is what
    /// stops the colony from spreading as a featureless circle.
    ///
    /// `u1` is floored away from zero because `ln(0)` is `-inf`; the clamp is
    /// one part in 8.4 million, so it does not bias the distribution.
    #[inline]
    pub fn normal(&mut self) -> f32 {
        let u1 = self.unit().max(1.0e-7);
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
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
        assert!(
            (sum / N as f64).abs() < 0.01,
            "mean drifted: {}",
            sum / N as f64
        );
    }

    /// A normal draw has to be centred, have unit-ish spread, and - the reason
    /// it exists - occasionally produce values far from the mean.
    #[test]
    fn normal_is_standard_and_has_tails() {
        let mut rng = SimRng::new(11);
        const N: usize = 200_000;
        let mut sum = 0.0f64;
        let mut sum_sq = 0.0f64;
        let mut beyond_two = 0usize;
        let mut extreme = 0.0f32;
        for _ in 0..N {
            let z = rng.normal();
            assert!(z.is_finite(), "normal() produced {z}");
            sum += z as f64;
            sum_sq += (z * z) as f64;
            if z.abs() > 2.0 {
                beyond_two += 1;
            }
            extreme = extreme.max(z.abs());
        }
        let mean = sum / N as f64;
        let var = sum_sq / N as f64 - mean * mean;
        assert!(mean.abs() < 0.01, "mean {mean}");
        assert!((var - 1.0).abs() < 0.02, "variance {var}");
        // ~4.6% of a standard normal lies beyond +/-2 sigma.
        let fraction = beyond_two as f64 / N as f64;
        assert!(
            (0.03..0.07).contains(&fraction),
            "{fraction} of draws beyond 2 sigma"
        );
        assert!(extreme > 4.0, "no tail at all: max |z| = {extreme}");
    }
}
