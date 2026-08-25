// SPDX-License-Identifier: GPL-2.0
//! Deterministic pseudo-random number generation for the kernel rewrite's
//! test suites.
//!
//! Design goals (in priority order):
//! 1. **Determinism** — same seed, same sequence, on every platform and
//!    every run. Only wrapping integer arithmetic is used; no floats, no
//!    system entropy, no time.
//! 2. **Simplicity / auditability** — two well-known algorithms: SplitMix64
//!    (seed expansion) and the canonical Xorshift128+ (Marsaglia) core.
//!    These are *not* cryptographic; never use them for security purposes
//!    (the kernel's own RNG lives in `drivers/char/random.c`, which has no
//!    business being re-implemented here).
//! 3. **Ergonomics** — a [`Rng`] trait with the helpers test code actually
//!    wants (`below` with rejection sampling, `shuffle`, `fill_bytes`),
//!    so crates stop hand-rolling xorshift closures.
//!
//! # C correspondence
//!
//! This crate has no direct C counterpart; it supports the ported test
//! suites (the kernel's KUnit tests use their own `rnd()` helpers, e.g.
//! `lib/tests/test_list_sort.c`).

#![no_std]
#![deny(unsafe_code)]

mod rng;
pub use rng::{derive_seed, Rng};

/// SplitMix64 stream generator (Steele & Lea, 2014).
///
/// Used to expand one user-provided `u64` seed into the two words of state
/// that Xorshift128+ needs, and available on its own as a fast, high-quality
/// 1-dimensional generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Creates a generator from an arbitrary seed (0 included).
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Advances the internal state and returns the next output.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl Rng for SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        SplitMix64::next_u64(self)
    }
}

/// Xorshift128+ (Marsaglia), the canonical 64-bit variant used by
/// xoroshiro's predecessor in V8/JS engines.
///
/// State must not be `(0, 0)`; use [`Krand::seed_from_u64`] rather than
/// constructing raw state by hand unless you know both words are non-zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Krand {
    s0: u64,
    s1: u64,
}

impl Krand {
    /// Creates a generator directly from two non-zero state words.
    ///
    /// Panics if both words are zero — Xorshift128+ is stuck at zero
    /// forever, and silently returning zeros would weaken every test using
    /// this crate.
    pub const fn from_state(s0: u64, s1: u64) -> Self {
        assert!((s0 | s1) != 0, "xorshift128+ state must not be all-zero");
        Self { s0, s1 }
    }

    /// Convenience constructor: deterministically expands `seed` into
    /// Xorshift128+ state via SplitMix64. This is what test code should use.
    pub fn seed_from_u64(seed: u64) -> Self {
        let mut sm = SplitMix64::new(seed);
        let s0 = sm.next_u64();
        let s1 = sm.next_u64();
        // s0 == s1 == 0 can only happen if splitmix produced two zero words;
        // mixing in the golden-ratio constant keeps the invariant trivially.
        Self::from_state(
            s0,
            if s0 | s1 == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                s1
            },
        )
    }
}

impl Rng for Krand {
    fn next_u64(&mut self) -> u64 {
        // Canonical Marsaglia xorshift128+ step.
        let x = self.s0;
        let y = self.s1;
        self.s0 = y;
        let mut x2 = x;
        x2 ^= x2 << 23;
        self.s1 = x2 ^ y ^ (x2 >> 17) ^ (y >> 26);
        self.s1.wrapping_add(y)
    }
}

#[cfg(test)]
mod tests;
