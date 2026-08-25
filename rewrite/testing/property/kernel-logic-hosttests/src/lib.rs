//! Host-side property testing over pure kernel logic.
//!
//! Includes the canonical `kernel_core.rs` verbatim via `#[path]` — the same
//! file the kernel builds. The mini-framework below (deterministic splitmix64
//! generation + bisection shrinking) is a stand-in for proptest/quickcheck so
//! this suite runs offline; swap it in when registry access exists.

#[path = "kernel_core.rs"]
mod kernel_core;

use kernel_core::{roundup_pow2, roundup_pow2_ref, RingCursor};

// ---------- mini property framework (offline stand-in for proptest) ---------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E3779B97F4A7C15)
    }
    fn next(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    fn u32_in(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next() % ((hi - lo) as u64 + 1)) as u32
    }
}

/// Run `cases` generated inputs through `prop`; on failure, shrink by
/// bisecting toward smaller values and report the minimal failing input.
fn check_u32_prop(
    name: &str,
    seed: u64,
    cases: u32,
    gen_hi: u32,
    prop: impl Fn(u32) -> Result<(), String>,
    shrinkable: bool,
) {
    let mut rng = Rng::new(seed);
    for _ in 0..cases {
        let x = rng.u32_in(1, gen_hi);
        if let Err(e) = prop(x) {
            let mut bad = x;
            if shrinkable {
                let mut lo = 1u32;
                while lo < bad {
                    let mid = lo + (bad - lo) / 2;
                    if prop(mid).is_err() {
                        bad = mid;
                    } else {
                        lo = mid + 1;
                    }
                }
                // confirm shrunk case still fails
                debug_assert!(prop(bad).is_err());
            }
            panic!("{name} FAILED at x={bad} (from x={x}): {e}");
        }
    }
}

const CASES: u32 = 25_000;

// ------------------------------ properties ----------------------------------

#[test]
fn prop_roundup_is_upper_bound() {
    // Contract domain: within range [1, 2^31] the result is >= input.
    // Above 2^31 the function saturates (see prop_roundup_saturates).
    check_u32_prop(
        "roundup>=x",
        0xA1,
        CASES,
        (u32::MAX / 2) + 1,
        |x| {
            let r = roundup_pow2(x);
            if r >= x {
                Ok(())
            } else {
                Err(format!("roundup_pow2({x}) = {r} < x"))
            }
        },
        true,
    );
}

#[test]
fn prop_roundup_saturates() {
    // Beyond the representable power of two, saturate instead of wrapping
    // (kernel's C version is UB here; ours must be total).
    check_u32_prop(
        "saturate",
        0xE7,
        CASES,
        u32::MAX,
        |x| {
            if x <= u32::MAX / 2 {
                return Ok(()); // below saturation domain
            }
            let r = roundup_pow2(x);
            if r == (u32::MAX / 2) + 1 {
                Ok(())
            } else {
                Err(format!(
                    "roundup_pow2({x}) = {r}, expected saturation at 2^31"
                ))
            }
        },
        false,
    );
}

#[test]
fn prop_roundup_is_power_of_two() {
    check_u32_prop(
        "popcount==1",
        0xB2,
        CASES,
        u32::MAX,
        |x| {
            let r = roundup_pow2(x);
            if r.count_ones() == 1 || r == 0 {
                Ok(())
            } else {
                Err(format!("roundup_pow2({x}) = {r} not a power of two"))
            }
        },
        true,
    );
}

#[test]
fn prop_roundup_matches_reference_oracle() {
    check_u32_prop(
        "oracle",
        0xC3,
        CASES,
        1 << 20,
        |x| {
            let fast = roundup_pow2(x);
            let slow = roundup_pow2_ref(x);
            if fast == slow {
                Ok(())
            } else {
                Err(format!("mismatch: {fast} != reference {slow}"))
            }
        },
        true,
    );
}

#[test]
fn prop_roundup_idempotent() {
    check_u32_prop(
        "idempotent",
        0xD4,
        CASES,
        u32::MAX,
        |x| {
            let once = roundup_pow2(x);
            if roundup_pow2(once) == once {
                Ok(())
            } else {
                Err("f(f(x)) != f(x)".into())
            }
        },
        false,
    );
}

/// Ring cursor: distance between two cursors never exceeds capacity-1,
/// regardless of how far either has advanced (absolute counters may wrap).
#[test]
fn prop_ring_distance_bounded() {
    let mut rng = Rng::new(0xE5);
    for _ in 0..CASES {
        let cap_exp = rng.u32_in(0, 15); // capacity = 2^cap_exp
        let cap = 1u32 << cap_exp;
        let w = RingCursor::new(cap).unwrap();
        let mut r = RingCursor::new(cap).unwrap();
        let mut wc = w;
        wc.advance(rng.next() % (1 << 40));
        r.advance(rng.next() % (1 << 40));
        let d = wc.distance(&r);
        assert!(
            d <= cap as u64 - 1,
            "distance {d} exceeds capacity-1 for cap={cap}"
        );
        assert_eq!(wc.index(), (wc.index()) & (cap - 1));
        let _ = &mut r;
    }
}

/// Advancing then retreating by the same amount restores position exactly.
#[test]
fn prop_ring_advance_retreat_inverse() {
    let mut rng = Rng::new(0xF6);
    for _ in 0..CASES {
        let cap = 1u32 << rng.u32_in(0, 12);
        let mut c = RingCursor::new(cap).unwrap();
        let n = rng.next() % 100_000;
        let before = c.index();
        c.advance(n);
        // retreat = advance by 2^64 - n
        c.advance(u64::MAX - n + 1);
        assert_eq!(c.index(), before, "advance/retreat not inverse");
    }
}

// --------------------------- known edge cases -------------------------------

#[test]
fn unit_edge_cases() {
    assert_eq!(roundup_pow2(0), 0);
    assert_eq!(roundup_pow2(1), 1);
    assert_eq!(roundup_pow2(2), 2);
    assert_eq!(roundup_pow2(3), 4);
    assert_eq!(roundup_pow2(513), 1024);
    assert_eq!(roundup_pow2(1 << 31), 1 << 31); // saturates instead of UB
    assert_eq!(RingCursor::new(3), None); // non-power-of-two rejected
    assert_eq!(RingCursor::new(0), None);
}
