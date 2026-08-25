//! Pure kernel decision logic — canonical location for host-side testing.
//!
//! In the tree this file lives next to its driver/subsystem and is compiled
//! into the kernel as a plain Rust module (`no_std`, no allocations, no
//! panics beyond arithmetic-checked paths). The host test crate includes this
//! exact file via `#[path]`, guaranteeing zero drift between tested and
//! shipped code.
//!
//! Demo logic chosen deliberately: power-of-two rounding and ring-buffer
//! wraparound math are among the most historically bug-dense patterns in C
//! drivers — exactly what property testing + model checking should own.

/// Smallest power of two >= `x`, saturating like the kernel's
/// `roundup_pow_of_two()` (which is UB for x > 2^31 on 32-bit; here it is
/// total and checked).
pub const fn roundup_pow2(x: u32) -> u32 {
    if x <= 1 {
        return x;
    }
    let high = u32::MAX / 2 + 1; // 2^31
    if x > high {
        return high; // kernel version would overflow; we saturate
    }
    let mut v = x - 1;
    v |= v >> 1;
    v |= v >> 2;
    v |= v >> 4;
    v |= v >> 8;
    v |= v >> 16;
    v + 1
}

/// Reference oracle: obvious O(log x) loop. Slower but trivially correct.
pub fn roundup_pow2_ref(mut x: u32) -> u32 {
    if x <= 1 {
        return x;
    }
    let mut p = 1u32;
    while p < x {
        // Saturate at the same point as the bit-twiddling version.
        if p > u32::MAX / 2 {
            return u32::MAX / 2 + 1;
        }
        p *= 2;
    }
    let _ = &mut x;
    p
}

/// Mask-indexed ring buffer cursor (capacity MUST be a power of two).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingCursor {
    /// Absolute position counter; only `pos & mask` is meaningful.
    pos: u64,
    mask: u64,
}

impl RingCursor {
    /// Panics (checked) if capacity is not a power of two — callers in the
    /// kernel use `build_assert!`; here we make the contract explicit.
    pub fn new(capacity_nonzero_pow2: u32) -> Option<Self> {
        let c = capacity_nonzero_pow2 as u64;
        if c == 0 || (c & (c - 1)) != 0 {
            return None;
        }
        Some(RingCursor {
            pos: 0,
            mask: c - 1,
        })
    }

    pub fn index(&self) -> u32 {
        (self.pos & self.mask) as u32
    }

    /// Advance by `n` slots. Overflow of the absolute counter cannot corrupt
    /// the index because the mask is applied modulo 2^k and u64 wraps
    /// cleanly at multiples of any 2^k <= 2^63.
    pub fn advance(&mut self, n: u64) {
        self.pos = self.pos.wrapping_add(n);
    }

    /// Slots free between writer `self` and reader `other`, given capacity.
    pub fn distance(&self, other: &RingCursor) -> u64 {
        self.pos.wrapping_sub(other.pos) & self.mask
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn roundup_is_upper_bound() {
        let x: u32 = kani::any();
        kani::assume(x >= 1 && x <= (u32::MAX / 2) + 1);
        let r = roundup_pow2(x);
        assert!(r >= x);
    }

    #[kani::proof]
    fn roundup_is_minimal() {
        let x: u32 = kani::any();
        kani::assume(x >= 2);
        let r = roundup_pow2(x);
        // u64 math: 2*x cannot overflow, so the assertion itself stays
        // well-defined across the whole input domain (harness hygiene).
        assert!(
            (r as u64) < 2 * (x as u64),
            "not the smallest such power of two"
        );
        assert!(r.count_ones() == 1);
    }

    #[kani::proof]
    fn ring_distance_never_exceeds_capacity() {
        let cap: u32 = kani::any();
        kani::assume(cap >= 1 && cap.count_ones() == 1);
        let w_pos: u64 = kani::any();
        let r_pos: u64 = kani::any();
        let mut w = RingCursor::new(cap).unwrap();
        let mut r = RingCursor::new(cap).unwrap();
        w.pos = w_pos;
        r.pos = r_pos;
        assert!(w.distance(&r) <= cap as u64 - 1);
        let _ = &mut w.advance(0);
    }
}
