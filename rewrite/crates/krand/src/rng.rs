//! The [`Rng`] trait: the helper vocabulary test code uses on top of any
//! generator producing `u64`s.

/// Random source. Implement [`Rng::next_u64`]; everything else is derived
/// with default methods (rejection sampling, unbiased).
pub trait Rng {
    /// Next raw 64-bit value.
    fn next_u64(&mut self) -> u64;

    /// Next raw 32-bit value (high half of a 64-bit draw — the high bits of
    /// an xorshift output are the well-mixed ones; SplitMix64 is fully mixed
    /// either way).
    fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform value in `0..upper_bound`, rejection-sampled.
    ///
    /// Panics if `upper_bound == 0`.
    ///
    /// Rejection: values from the tail of `0..2^64` that would bias
    /// `r % upper_bound` are discarded, so the distribution is exactly
    /// uniform regardless of how `upper_bound` divides `2^64`. Rejection is
    /// bounded: at most ~2 expected redraws for any bound.
    fn below(&mut self, upper_bound: u64) -> u64 {
        assert!(upper_bound != 0, "below(0) has no valid answer");
        // threshold = 2^64 mod upper_bound: draws below it are the biased tail.
        let threshold = upper_bound.wrapping_neg() % upper_bound;
        loop {
            let r = self.next_u64();
            if r >= threshold {
                return r % upper_bound;
            }
        }
    }

    /// Uniform `usize` convenience wrapper around [`Rng::below`].
    fn below_usize(&mut self, upper_bound: usize) -> usize {
        self.below(upper_bound as u64) as usize
    }

    /// Unbiased coin flip.
    fn coin_flip(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// Fisher–Yates shuffle, uniformly over all permutations.
    fn shuffle<T>(&mut self, slice: &mut [T]) {
        let n = slice.len();
        for i in (1..n).rev() {
            let j = self.below_usize(i + 1);
            slice.swap(i, j);
        }
    }

    /// Fills `dest` with random bytes (LE chunks of one 64-bit draw each).
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_exact_mut(8) {
            chunk.copy_from_slice(&self.next_u64().to_le_bytes());
        }
        let rest = dest.len() % 8;
        if rest != 0 {
            let bytes = self.next_u64().to_le_bytes();
            let tail = dest.len() - rest;
            dest[tail..].copy_from_slice(&bytes[..rest]);
        }
    }
}

/// Blanket impl: anything that can produce `u64`s gets the helpers by
/// delegating through its own `next_u64`.
impl<T: Rng> Rng for &mut T {
    fn next_u64(&mut self) -> u64 {
        T::next_u64(self)
    }
}

/// Derives a fresh per-iteration seed from a base seed and iteration index.
///
/// Public because `ktest-util::run_fuzz` and hand-rolled loops share it:
/// iterations stay independent yet reproducible from `(base_seed, i)` alone.
pub fn derive_seed(base_seed: u64, iteration: u64) -> u64 {
    base_seed.wrapping_add(iteration.wrapping_mul(0x9E37_79B9_7F4A_7C15))
}
