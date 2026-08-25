// SPDX-License-Identifier: GPL-2.0
//! Kani proof harnesses for `kmath64` (compiled only under `--cfg kani`).
//!
//! Run with: cargo kani -p kmath64 --default-unwind <N>
//!
//! These harnesses prove properties over ALL symbolic inputs within their
//! assumed bounds (bounded model checking), complementing the differential
//! test-suite in `tests.rs`:
//!
//! * [`div_u64_rem_panic_freedom_and_spec`] — no panic for any dividend with
//!   a non-zero divisor, and bit-exact agreement with native division.
//! * [`gcd_panic_freedom_and_divides_both`] — no panic for any input pair,
//!   and the result divides both operands.
//! * [`reciprocal_divide_matches_native_for_all_u32`] — the precomputed
//!   reciprocal reproduces native 32-bit division exactly for every dividend.

#[cfg(kani)]
mod verify {
    use crate::div_u64;
    use crate::div_u64_rem;
    use crate::gcd;
    use crate::reciprocal_divide;
    use crate::reciprocal_value;

    #[kani::proof]
    fn div_u64_rem_panic_freedom_and_spec() {
        let n: u64 = kani::any();
        let d: u32 = kani::any();
        kani::assume(d != 0); // divisor 0 is a documented panic contract
        let dn = u64::from(d);
        assert_eq!(div_u64_rem(n, d), (n / dn, (n % dn) as u32));
        assert_eq!(div_u64(n, d), n / dn);
    }

    #[kani::proof]
    fn gcd_panic_freedom_and_divides_both() {
        let a: u64 = kani::any();
        let b: u64 = kani::any();
        let g = gcd(a, b);
        if a != 0 {
            assert_eq!(a % g, 0, "gcd must divide a");
        }
        if b != 0 {
            assert_eq!(b % g, 0, "gcd must divide b");
        }
    }

    #[kani::proof]
    fn reciprocal_divide_matches_native_for_all_u32() {
        let a: u32 = kani::any();
        let d: u32 = kani::any();
        kani::assume(d != 0);
        let r = reciprocal_value(d);
        // The multiply-shift scheme must equal direct division for every
        // dividend (Granlund & Montgomery correctness for this domain).
        assert_eq!(reciprocal_divide(a, r), a / d);
    }
}
