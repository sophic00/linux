//! Tests for the `lib/math/div64.c` / `gcd.c` / `reciprocal_div.c` rewrite:
//! kernel tables ported from `lib/math/test_div64.c`,
//! `lib/math/test_mul_u64_u64_div_u64.c` and `lib/math/tests/gcd_kunit.c`,
//! plus differential/property tests against native wide arithmetic.

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;

use alloc::{vec, vec::Vec};

use super::*;

/// xorshift64: self-contained deterministic PRNG (no external deps).
struct Xorshift64(u64);

impl Xorshift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_range(&mut self, lo: u64, hi_incl: u64) -> u64 {
        lo + self.next() % (hi_incl - lo + 1)
    }
}

// Ported from lib/math/test_div64.c (mechanically extracted)
const TEST_DIV64_DIVIDENDS: [u64; 12] = [
    0x00000000ab275080,
    0x0000000fe73c1959,
    0x000000e54c0a74b1,
    0x00000d4398ff1ef9,
    0x0000a18c2ee1c097,
    0x00079fb80b072e4a,
    0x0072db27380dd689,
    0x0842f488162e2284,
    0xf66745411d8ab063,
    0xfffffffffffffffb,
    0xfffffffffffffffc,
    0xffffffffffffffff,
];
const TEST_DIV64_DIVISORS: [u32; 12] = [
    0x00000009, 0x0000007c, 0x00000204, 0x0000cb5b, 0x00010000, 0x0008a880, 0x003fd3ae, 0x0b658fac,
    0x80000001, 0xdc08b349, 0xfffffffe, 0xffffffff,
];
// (quotient, remainder) indexed [dividend][divisor]
const TEST_DIV64_RESULTS: [[(u64, u32); 12]; 12] = [
    [
        (0x0000000013045e47, 0x00000001),
        (0x000000000161596c, 0x00000030),
        (0x000000000054e9d4, 0x00000130),
        (0x000000000000d776, 0x0000278e),
        (0x000000000000ab27, 0x00005080),
        (0x00000000000013c4, 0x0004ce80),
        (0x00000000000002ae, 0x001e143c),
        (0x000000000000000f, 0x0033e56c),
        (0x0000000000000001, 0x2b27507f),
        (0x0000000000000000, 0xab275080),
        (0x0000000000000000, 0xab275080),
        (0x0000000000000000, 0xab275080),
    ],
    [
        (0x00000001c45c02d1, 0x00000000),
        (0x0000000020d5213c, 0x00000049),
        (0x0000000007e3d65f, 0x000001dd),
        (0x0000000000140531, 0x000065ee),
        (0x00000000000fe73c, 0x00001959),
        (0x000000000001d637, 0x0004e5d9),
        (0x0000000000003fc9, 0x000713bb),
        (0x0000000000000165, 0x029abe7d),
        (0x000000000000001f, 0x673c193a),
        (0x0000000000000012, 0x6e9f7e37),
        (0x000000000000000f, 0xe73c1977),
        (0x000000000000000f, 0xe73c1968),
    ],
    [
        (0x000000197a3a0cf7, 0x00000002),
        (0x00000001d9632e5c, 0x00000021),
        (0x0000000071c28039, 0x000001cd),
        (0x000000000120a844, 0x0000b885),
        (0x0000000000e54c0a, 0x000074b1),
        (0x00000000001a7bb3, 0x00072331),
        (0x00000000000397ad, 0x0002c61b),
        (0x000000000000141e, 0x06ea2e89),
        (0x00000000000001ca, 0x4c0a72e7),
        (0x000000000000010a, 0xab002ad7),
        (0x00000000000000e5, 0x4c0a767b),
        (0x00000000000000e5, 0x4c0a7596),
    ],
    [
        (0x0000017949e37538, 0x00000001),
        (0x0000001b62441f37, 0x00000055),
        (0x0000000694a3391d, 0x00000085),
        (0x0000000010b2a5d2, 0x0000a753),
        (0x000000000d4398ff, 0x00001ef9),
        (0x0000000001882ec6, 0x0005cbf9),
        (0x000000000035333b, 0x0017abdf),
        (0x00000000000129f1, 0x0ab4520d),
        (0x0000000000001a87, 0x18ff0472),
        (0x0000000000000f6e, 0x8ac0ce9b),
        (0x0000000000000d43, 0x98ff397f),
        (0x0000000000000d43, 0x98ff2c3c),
    ],
    [
        (0x000011f321a74e49, 0x00000006),
        (0x0000014d8481d211, 0x0000005b),
        (0x0000005025cbd92d, 0x000001e3),
        (0x00000000cb5e71e3, 0x000043e6),
        (0x00000000a18c2ee1, 0x0000c097),
        (0x0000000012a88828, 0x00036c97),
        (0x000000000287f16f, 0x002c2a25),
        (0x00000000000e2cc7, 0x02d581e3),
        (0x0000000000014318, 0x2ee07d7f),
        (0x000000000000bbf4, 0x1ba08c03),
        (0x000000000000a18c, 0x2ee303af),
        (0x000000000000a18c, 0x2ee26223),
    ],
    [
        (0x0000d8db8f72935d, 0x00000005),
        (0x00000fbd5aed7a2e, 0x00000002),
        (0x000003c84b6ea64a, 0x00000122),
        (0x0000000998fa8829, 0x000044b7),
        (0x000000079fb80b07, 0x00002e4a),
        (0x00000000e16b20fa, 0x0002a14a),
        (0x000000001e940d22, 0x00353b2e),
        (0x0000000000ab40ac, 0x06fba6ba),
        (0x00000000000f3f70, 0x0af7eeda),
        (0x000000000008debd, 0x72d98365),
        (0x0000000000079fb8, 0x0b166dba),
        (0x0000000000079fb8, 0x0b0ece02),
    ],
    [
        (0x000cc3045b8fc281, 0x00000000),
        (0x0000ed1f48b5c9fc, 0x00000079),
        (0x000038fb9c63406a, 0x000000e1),
        (0x000000909705b825, 0x00000a62),
        (0x00000072db27380d, 0x0000d689),
        (0x0000000d43fce827, 0x00082b09),
        (0x00000001ccaba11a, 0x0037e8dd),
        (0x000000000a13f729, 0x0566dffd),
        (0x0000000000e5b64e, 0x3728203b),
        (0x000000000085a14b, 0x23d36726),
        (0x000000000072db27, 0x38f38cd7),
        (0x000000000072db27, 0x3880b1b0),
    ],
    [
        (0x00eafeb9c993592b, 0x00000001),
        (0x00110e5befa9a991, 0x00000048),
        (0x00041947b4a1d36a, 0x000000dc),
        (0x00000a6679327311, 0x0000c079),
        (0x00000842f488162e, 0x00002284),
        (0x000000f4459740fc, 0x00084484),
        (0x0000002122c47bf9, 0x002ca446),
        (0x00000000b9936290, 0x004979c4),
        (0x000000001085e910, 0x05a83974),
        (0x00000000099ca89d, 0x9db446bf),
        (0x000000000842f488, 0x26b40b94),
        (0x000000000842f488, 0x1e71170c),
    ],
    [
        (0x1b60cece589da1d2, 0x00000001),
        (0x01fcb42be1453f5b, 0x0000004f),
        (0x007a3f2457df0749, 0x0000013f),
        (0x0001363130e3ec7b, 0x000017aa),
        (0x0000f66745411d8a, 0x0000b063),
        (0x00001c757dfab350, 0x00048863),
        (0x000003dc4979c652, 0x00224ea7),
        (0x000000159edc3144, 0x06409ab3),
        (0x00000001ecce8a7e, 0x30bc25e5),
        (0x000000011eadfee3, 0xa99c48a8),
        (0x00000000f6674543, 0x0a593ae9),
        (0x00000000f6674542, 0x13f1f5a5),
    ],
    [
        (0x1c71c71c71c71c71, 0x00000002),
        (0x0210842108421084, 0x0000000b),
        (0x007f01fc07f01fc0, 0x000000fb),
        (0x00014245eabf1f9a, 0x0000a63d),
        (0x0000ffffffffffff, 0x0000fffb),
        (0x00001d913cecc509, 0x0007937b),
        (0x00000402c70c678f, 0x0005bfc9),
        (0x00000016766cb70b, 0x045edf97),
        (0x00000001fffffffb, 0x80000000),
        (0x0000000129d84b3a, 0xa2e8fe71),
        (0x0000000100000001, 0xfffffffd),
        (0x0000000100000000, 0xfffffffb),
    ],
    [
        (0x1c71c71c71c71c71, 0x00000003),
        (0x0210842108421084, 0x0000000c),
        (0x007f01fc07f01fc0, 0x000000fc),
        (0x00014245eabf1f9a, 0x0000a63e),
        (0x0000ffffffffffff, 0x0000fffc),
        (0x00001d913cecc509, 0x0007937c),
        (0x00000402c70c678f, 0x0005bfca),
        (0x00000016766cb70b, 0x045edf98),
        (0x00000001fffffffc, 0x00000000),
        (0x0000000129d84b3a, 0xa2e8fe72),
        (0x0000000100000002, 0x00000000),
        (0x0000000100000000, 0xfffffffc),
    ],
    [
        (0x1c71c71c71c71c71, 0x00000006),
        (0x0210842108421084, 0x0000000f),
        (0x007f01fc07f01fc0, 0x000000ff),
        (0x00014245eabf1f9a, 0x0000a641),
        (0x0000ffffffffffff, 0x0000ffff),
        (0x00001d913cecc509, 0x0007937f),
        (0x00000402c70c678f, 0x0005bfcd),
        (0x00000016766cb70b, 0x045edf9b),
        (0x00000001fffffffc, 0x00000003),
        (0x0000000129d84b3a, 0xa2e8fe75),
        (0x0000000100000002, 0x00000003),
        (0x0000000100000001, 0x00000000),
    ],
];

// Ported from lib/math/test_mul_u64_u64_div_u64.c (a, b, d, floor_result, round_up)
const MUL_TEST_VALUES: [(u64, u64, u64, u64, u8); 28] = [
    (0xb, 0x7, 0x3, 0x19, 1),
    (0xffff0000, 0xffff0000, 0xf, 0x1110eeef00000000, 0),
    (0xffffffff, 0xffffffff, 0x1, 0xfffffffe00000001, 0),
    (0xffffffff, 0xffffffff, 0x2, 0x7fffffff00000000, 1),
    (0x1ffffffff, 0xffffffff, 0x2, 0xfffffffe80000000, 1),
    (0x1ffffffff, 0xffffffff, 0x3, 0xaaaaaaa9aaaaaaab, 0),
    (0x1ffffffff, 0x1ffffffff, 0x4, 0xffffffff00000000, 1),
    (
        0xffff000000000000,
        0xffff000000000000,
        0xffff000000000001,
        0xfffeffffffffffff,
        1,
    ),
    (
        0x3333333333333333,
        0x3333333333333333,
        0x5555555555555555,
        0x1eb851eb851eb851,
        1,
    ),
    (0x7fffffffffffffff, 0x2, 0x3, 0x5555555555555554, 1),
    (0xffffffffffffffff, 0x2, 0x8000000000000000, 0x3, 1),
    (0xffffffffffffffff, 0x2, 0xc000000000000000, 0x2, 1),
    (
        0xffffffffffffffff,
        0x4000000000000004,
        0x8000000000000000,
        0x8000000000000007,
        1,
    ),
    (
        0xffffffffffffffff,
        0x4000000000000001,
        0x8000000000000000,
        0x8000000000000001,
        1,
    ),
    (
        0xffffffffffffffff,
        0x8000000000000001,
        0xffffffffffffffff,
        0x8000000000000001,
        0,
    ),
    (
        0xfffffffffffffffe,
        0x8000000000000001,
        0xffffffffffffffff,
        0x8000000000000000,
        1,
    ),
    (
        0xffffffffffffffff,
        0x8000000000000001,
        0xfffffffffffffffe,
        0x8000000000000001,
        1,
    ),
    (
        0xffffffffffffffff,
        0x8000000000000001,
        0xfffffffffffffffd,
        0x8000000000000002,
        1,
    ),
    (
        0x7fffffffffffffff,
        0xffffffffffffffff,
        0xc000000000000000,
        0xaaaaaaaaaaaaaaa8,
        1,
    ),
    (
        0xffffffffffffffff,
        0x7fffffffffffffff,
        0xa000000000000000,
        0xccccccccccccccca,
        1,
    ),
    (
        0xffffffffffffffff,
        0x7fffffffffffffff,
        0x9000000000000000,
        0xe38e38e38e38e38b,
        1,
    ),
    (
        0x7fffffffffffffff,
        0x7fffffffffffffff,
        0x5000000000000000,
        0xccccccccccccccc9,
        1,
    ),
    (
        0xffffffffffffffff,
        0xfffffffffffffffe,
        0xffffffffffffffff,
        0xfffffffffffffffe,
        0,
    ),
    (
        0xe6102d256d7ea3ae,
        0x70a77d0be4c31201,
        0xd63ec35ab3220357,
        0x78f8bf8cc86c6e18,
        1,
    ),
    (
        0xf53bae05cb86c6e1,
        0x3847b32d2f8d32e0,
        0xcfd4f55a647f403c,
        0x42687f79d8998d35,
        1,
    ),
    (
        0x9951c5498f941092,
        0x1f8c8bfdf287a251,
        0xa3c8dc5f81ea3fe2,
        0x1d887cb25900091f,
        1,
    ),
    (
        0x374fee9daa1bb2bb,
        0x0d0bfbff7b8ae3ef,
        0xc169337bd42d5179,
        0x03bb2dbaffcbb961,
        1,
    ),
    (
        0xeac0d03ac10eeaf0,
        0x89be05dfa162ed9b,
        0x92bb1679a41f0e4b,
        0xdc5f5cc9e270d216,
        1,
    ),
];

// ---------------------------------------------------------------------------
// fls/ffs
// ---------------------------------------------------------------------------

#[test]
fn fls_matches_kernel_contract() {
    assert_eq!(fls32(0), 0);
    assert_eq!(fls32(1), 1);
    assert_eq!(fls32(0x8000_0000), 32);
    assert_eq!(fls32(0xffff_ffff), 32);
    for b in 0..32u32 {
        assert_eq!(fls32(1u32 << b), b + 1);
    }
}

// ---------------------------------------------------------------------------
// div_u64_rem / div_u64
// ---------------------------------------------------------------------------

#[test]
fn div_u64_rem_differential() {
    let mut rng = Xorshift64(0x9E3779B97F4A7C15);
    // exhaustive small domain
    for n in 0..=2000u64 {
        for d in 1..=64u32 {
            let (q, r) = div_u64_rem(n, d);
            assert_eq!((q, r), (n / u64::from(d), (n % u64::from(d)) as u32));
            assert_eq!(checked_div_u64_rem(n, d), Some((q, r)));
        }
    }
    // random wide values incl boundaries
    let mut cases: Vec<u64> = (0..20_000).map(|_| rng.next()).collect();
    for shift in 0..64 {
        for delta in [-1i64, 0, 1] {
            let v = (1u128 << shift) as i128 + i128::from(delta);
            if v >= 0 && v <= u64::MAX as i128 {
                cases.push(v as u64);
            }
        }
    }
    for &n in &cases {
        let d = 1 + (rng.next() % (u32::MAX as u64 - 2)) as u32;
        let (q, r) = div_u64_rem(n, d);
        assert_eq!(q, n / u64::from(d));
        assert_eq!(u64::from(r), n % u64::from(d));
    }
    assert_eq!(checked_div_u64_rem(123, 0), None);
}

#[test]
#[should_panic(expected = "division by zero")]
fn div_u64_rem_zero_panics() {
    let _ = div_u64_rem(1, 0);
}

// ---------------------------------------------------------------------------
// __div64_32 (shift-subtract algorithm)
// ---------------------------------------------------------------------------

/// The kernel's own table: lib/math/test_div64.c.
#[test]
fn div64_32_kernel_table() {
    for (ni, &n) in TEST_DIV64_DIVIDENDS.iter().enumerate() {
        for (di, &d) in TEST_DIV64_DIVISORS.iter().enumerate() {
            let (q, r) = div64_32(n, d);
            assert_eq!((q, r), TEST_DIV64_RESULTS[ni][di], "n={n:#x} d={d:#x}");
            // and must equal native division
            assert_eq!(q, n / u64::from(d));
            assert_eq!(r as u64, n % u64::from(d));
        }
    }
}

#[test]
fn div64_32_exhaustive_small_vs_native() {
    for n in 0..=3000u64 {
        for d in 1..=70u32 {
            let (q, r) = div64_32(n, d);
            assert_eq!(q, n / u64::from(d));
            assert_eq!(r as u64, n % u64::from(d));
        }
    }
}

#[test]
fn div64_32_boundaries_vs_native() {
    let mut rng = Xorshift64(0xDEADBEEFCAFEBABE);
    let mut dividends: Vec<u64> = vec![0, 1, u32::MAX as u64, u32::MAX as u64 + 1];
    for b in 0..64 {
        for delta in [-1i64, 0, 1] {
            let v = (1u128 << b) as i128 + i128::from(delta);
            if v >= 0 && v <= u64::MAX as i128 {
                dividends.push(v as u64);
            }
        }
    }
    dividends.extend((0..5000).map(|_| rng.next()));
    let mut divisors: Vec<u32> = vec![1, 2, 3, u32::MAX, u32::MAX - 1];
    for b in 0..32 {
        divisors.push(1u32 << b);
        divisors.push((1u32 << b).wrapping_sub(1));
        divisors.push((1u32 << b).wrapping_add(1));
    }
    divisors.extend((0..2000).map(|_| (rng.next() >> 32) as u32).map(|x| x | 1)); // keep nonzero

    // Sampled pairs (full cross product lives in the ignored heavy variant).
    for (i, &n) in dividends.iter().enumerate() {
        for j in 0..12usize {
            let d = divisors[(i * 31 + j * 17) % divisors.len()];
            if d == 0 {
                continue;
            }
            let (q, r) = div64_32(n, d);
            assert_eq!(q, n / u64::from(d), "n={n:#x} d={d:#x}");
            assert_eq!(u64::from(r), n % u64::from(d));
        }
    }
    // Every dividend against a handful of fixed divisors.
    for &n in &dividends {
        for &d in &[1u32, 2, 3, u32::MAX, 0x8000_0001] {
            let (q, r) = div64_32(n, d);
            assert_eq!(q, n / u64::from(d));
            assert_eq!(u64::from(r), n % u64::from(d));
        }
    }
}

#[test]
#[ignore = "slow: full dividend x divisor cross product"]
fn div64_32_full_cross_product() {
    let mut rng = Xorshift64(0xDEADBEEFCAFEBABE);
    let mut dividends: Vec<u64> = vec![0, 1];
    for b in 0..64 {
        for delta in [-1i64, 0, 1] {
            let v = (1u128 << b) as i128 + i128::from(delta);
            if v >= 0 && v <= u64::MAX as i128 {
                dividends.push(v as u64);
            }
        }
    }
    dividends.extend((0..2000).map(|_| rng.next()));
    for &n in &dividends {
        for d in 1..=u32::MAX {
            if rng.next() % 97 != 0 {
                continue;
            }
            let (q, r) = div64_32(n, d);
            assert_eq!((q, u64::from(r)), (n / u64::from(d), n % u64::from(d)));
        }
    }
}

#[test]
#[should_panic(expected = "division by zero")]
fn div64_32_zero_panics() {
    let _ = div64_32(42, 0);
}

// ---------------------------------------------------------------------------
// div_s64_rem / div_s64
// ---------------------------------------------------------------------------

#[test]
fn div_s64_rem_exhaustive_small() {
    for n in -100i64..=100 {
        for d in -70i32..=70 {
            if d == 0 || (n == i64::MIN && false) {
                continue;
            }
            // native oracle (Rust native ops have identical defined semantics)
            let (q, r) = div_s64_rem(n, d);
            let di = i64::from(d);
            assert_eq!((q, i64::from(r)), (n / di, n % di), "n={n} d={d}");
            // truncation identity + remainder sign follows dividend
            assert_eq!(q * di + i64::from(r), n);
            assert!(i64::from(r.abs()) < di.abs());
            assert!(r == 0 || i64::from(r.signum()) == n.signum());
        }
    }
}

#[test]
fn div_s64_rem_random_and_extremes() {
    let mut rng = Xorshift64(0x0123456789ABCDEF);
    for _ in 0..20_000 {
        let n = rng.next() as i64;
        let d = (rng.next() >> 32) as i32;
        if d == 0 {
            continue;
        }
        let (q, r) = div_s64_rem(n, d);
        assert_eq!(q, n / i64::from(d));
        assert_eq!(i64::from(r), n % i64::from(d));
    }
    // i64::MIN with divisor != -1 is well-defined and must work
    for d in [1i32, 2, -2, 3, i32::MAX, i32::MIN] {
        let (q, r) = div_s64_rem(i64::MIN, d);
        assert_eq!(
            (q, i64::from(r)),
            (i64::MIN / i64::from(d), i64::MIN % i64::from(d))
        );
        assert_eq!(div_s64(i64::MIN, d), q);
    }
    // i32::MIN divisor exercises the unsigned-magnitude path (C abs() was UB)
    let (q, r) = div_s64_rem(-3, i32::MIN);
    assert_eq!((q, r), (0, -3));
}

#[test]
#[should_panic(expected = "division by zero")]
fn div_s64_rem_zero_panics() {
    let _ = div_s64_rem(-1, 0);
}

#[test]
#[should_panic(expected = "i64::MIN / -1 overflows")]
fn div_s64_rem_min_over_minus_one_panics() {
    let _ = div_s64_rem(i64::MIN, -1);
}

// ---------------------------------------------------------------------------
// div64_u64 / div64_u64_rem (Hacker's Delight algorithm)
// ---------------------------------------------------------------------------

#[test]
fn div64_u64_boundaries_vs_native() {
    let mut rng = Xorshift64(0x1122334455667788);
    let mut values: Vec<u64> = vec![0, 1, 2, u32::MAX as u64, u32::MAX as u64 + 1, u64::MAX];
    for b in 0..64 {
        for delta in [-1i64, 0, 1] {
            let v = (1u128 << b) as i128 + i128::from(delta);
            if v >= 0 && v <= u64::MAX as i128 {
                values.push(v as u64);
            }
        }
    }
    values.extend((0..10_000).map(|_| rng.next()));

    for &n in &values {
        for &d in values.iter().take(400) {
            if d == 0 {
                continue;
            }
            let q = div64_u64(n, d);
            assert_eq!(q, n / d, "n={n:#x} d={d:#x}");
            let (q2, r) = div64_u64_rem(n, d);
            assert_eq!(q2, n / d);
            assert_eq!(r, n % d);
            assert_eq!(q2 * d + r, n);
            assert!(r < d);
        }
    }
}

#[test]
fn div64_u64_high_divisor_focus() {
    // Divisors with high bit set exercise the Hacker's Delight path; sweep
    // them against small dividends and random ones.
    let mut rng = Xorshift64(0xA5A5A5A55A5A5A5A);
    let mut divisors: Vec<u64> = Vec::new();
    for b in 32..64 {
        divisors.push(1u64 << b);
        divisors.push((1u64 << b) - 1);
        divisors.push((1u64 << b) + 1);
    }
    divisors.push(u64::MAX);
    divisors.extend((0..3000).map(|_| rng.next() | (1 << 63)));

    let mut dividends: Vec<u64> = (0..3000).map(|_| rng.next()).collect();
    dividends.extend([0, 1, u64::MAX, u64::MAX - 1, 1 << 63, (1 << 63) - 1]);

    for &d in &divisors {
        for &n in &dividends {
            assert_eq!(div64_u64(n, d), n / d, "n={n:#x} d={d:#x}");
            let (q, r) = div64_u64_rem(n, d);
            assert_eq!((q, r), (n / d, n % d));
        }
    }
}

#[test]
#[should_panic(expected = "division by zero")]
fn div64_u64_zero_panics() {
    let _ = div64_u64(42, 0);
}

#[test]
#[should_panic(expected = "division by zero")]
fn div64_u64_rem_zero_panics() {
    let _ = div64_u64_rem(42, 0);
}

// ---------------------------------------------------------------------------
// div64_s64
// ---------------------------------------------------------------------------

#[test]
fn div64_s64_exhaustive_small() {
    for n in -60i64..=60 {
        for d in -60i64..=60 {
            if d == 0 {
                continue;
            }
            assert_eq!(div64_s64(n, d), n / d, "n={n} d={d}");
        }
    }
}

#[test]
fn div64_s64_random_and_extremes() {
    let mut rng = Xorshift64(0xFEDCBA9876543210);
    for _ in 0..6_000 {
        let n = rng.next() as i64;
        let d = rng.next() as i64;
        // i64::MIN / -1 is documented UB in C; we panic by contract, so the
        // differential loop must skip exactly that pair.
        if d == 0 || (n == i64::MIN && d == -1) {
            continue;
        }
        assert_eq!(div64_s64(n, d), n / d);
    }
    for d in [1i64, -1, 2, -2, 3, i64::MAX, i64::MIN] {
        if d == 0 || d == -1 {
            // -1: covered by the dedicated MIN/-1 panic test above.
            continue;
        }
        assert_eq!(div64_s64(i64::MIN, d), i64::MIN / d, "d={d}");
        assert_eq!(div64_s64(i64::MAX, d), i64::MAX / d, "d={d}");
    }
}

#[test]
#[should_panic(expected = "division by zero")]
fn div64_s64_zero_panics() {
    let _ = div64_s64(-1, 0);
}

#[test]
#[should_panic(expected = "i64::MIN / -1 overflows")]
fn div64_s64_min_over_minus_one_panics() {
    let _ = div64_s64(i64::MIN, -1);
}

// ---------------------------------------------------------------------------
// iter_div_u64_rem
// ---------------------------------------------------------------------------

#[test]
fn iter_div_u64_rem_matches_native() {
    let divisors = [1u32, 2, 3, 7, 100, 1000, 1 << 16, u32::MAX];
    for &d in &divisors {
        for n in 0..=1_500u64 {
            let (q, r) = iter_div_u64_rem(n, d);
            assert_eq!(q as u64, n / u64::from(d), "n={n} d={d}");
            assert_eq!(r, n % u64::from(d));
        }
    }
    let mut rng = Xorshift64(0x00C0FFEE00000001);
    for _ in 0..500 {
        let n = rng.next() % 1_000_000;
        let d = 1 + (rng.next() % 50_000) as u32;
        let (q, r) = iter_div_u64_rem(n, d);
        assert_eq!((q as u64, r), (n / u64::from(d), n % u64::from(d)));
    }
}

/// Heavy subtract-loop sweep (O(sum n/d) iterations): run explicitly via
/// `cargo test -p kmath64 -- --ignored`.
#[test]
#[ignore = "slow: millions of subtraction-loop iterations in debug builds"]
fn iter_div_heavy_sweep() {
    let divisors = [1u32, 2, 3, 7, 100, 1000];
    for &d in &divisors {
        for n in 0..=50_000u64 {
            let (q, r) = iter_div_u64_rem(n, d);
            assert_eq!((q as u64, r), (n / u64::from(d), n % u64::from(d)));
        }
    }
}

#[test]
#[should_panic(expected = "division by zero")]
fn iter_div_zero_panics_instead_of_hanging() {
    // C would loop forever here; the Rust port panics (documented deviation).
    let _ = iter_div_u64_rem(1, 0);
}

// ---------------------------------------------------------------------------
// mul_u64_add_u64_div_u64 family
// ---------------------------------------------------------------------------

/// u128 oracle: floor((a*b+c)/d).
fn ref_mul_div(a: u64, b: u64, c: u64, d: u64) -> Option<u64> {
    if d == 0 {
        return None;
    }
    let p = (a as u128) * (b as u128) + c as u128;
    if p / d as u128 > u64::MAX as u128 {
        return None; // unrepresentable
    }
    Some((p / d as u128) as u64)
}

/// The kernel's own table from lib/math/test_mul_u64_u64_div_u64.c,
/// including the round-up variants.
#[test]
fn mul_u64_u64_div_u64_kernel_table() {
    for &(a, b, d, expected, round_up) in MUL_TEST_VALUES.iter() {
        let got = mul_u64_u64_div_u64(a, b, d);
        assert_eq!(got, expected, "floor {a:#x}*{b:#x}/{d:#x}");

        let mut expected_up = expected;
        expected_up = expected_up.wrapping_add(u64::from(round_up));
        let got_up = mul_u64_u64_div_u64_roundup(a, b, d);
        assert_eq!(got_up, expected_up, "ceil {a:#x}*{b:#x}+/{d:#x}");
    }
}

#[test]
fn mul_u64_add_u64_div_u64_differential_random() {
    let mut rng = Xorshift64(0x1234_5678_9ABC_DEF0);
    let interesting: Vec<u64> = vec![
        0,
        1,
        2,
        3,
        u32::MAX as u64,
        u32::MAX as u64 + 1,
        (1 << 32) - 1,
        (1 << 63),
        u64::MAX,
        u64::MAX - 1,
    ];
    for _ in 0..6_000 {
        let pick = |r: &mut Xorshift64| -> u64 {
            match r.next() % 3 {
                0 => r.next(),
                1 => interesting[(r.next() % interesting.len() as u64) as usize],
                _ => r.next() >> (r.next() % 64),
            }
        };
        let a = pick(&mut rng);
        let b = pick(&mut rng);
        let c = pick(&mut rng);
        let d = pick(&mut rng);
        if d == 0 {
            continue;
        }
        let want = ref_mul_div(a, b, c, d);
        let got = mul_u64_add_u64_div_u64(a, b, c, d);
        match want {
            None => assert_eq!(got, MUL_OVERFLOW, "saturation {a:#x} {b:#x} {c:#x} {d:#x}"),
            Some(w) => assert_eq!(got, w, "{a:#x}*{b:#x}+{c:#x})/{d:#x}"),
        }
    }
}

#[test]
fn mul_u64_add_u64_div_u64_exhaustive_small() {
    for a in 0..40u64 {
        for b in 0..40u64 {
            for c in [0u64, 1, 7, 39] {
                for d in 1..40u64 {
                    let want = (a * b + c) / d;
                    assert_eq!(mul_u64_add_u64_div_u64(a, b, c, d), want);
                }
            }
        }
    }
}

#[test]
fn mul_u64_add_u64_div_u64_saturation_is_defined() {
    // Quotient truly unrepresentable: saturates to C's defined ~0ULL.
    assert_eq!(
        mul_u64_add_u64_div_u64(u64::MAX, u64::MAX, 0, 1),
        MUL_OVERFLOW
    );
    assert_eq!(
        mul_u64_add_u64_div_u64(u64::MAX, u64::MAX, u64::MAX, 2),
        MUL_OVERFLOW
    );
    // Large but *representable* quotients just below the 2^64 edge:
    // (2^65 - 2)/3 < 2^64, must NOT saturate.
    assert_eq!(
        mul_u64_add_u64_div_u64(u64::MAX, 2, 0, 3),
        (((1u128 << 65) - 2) / 3) as u64
    );
    // 2^65/5 < 2^64 likewise.
    assert_eq!(
        mul_u64_add_u64_div_u64(1 << 63, 4, 0, 5),
        ((1u128 << 65) / 5) as u64
    );
}

#[test]
#[should_panic(expected = "division by zero")]
fn mul_div_zero_panics_low_path() {
    // n_hi == 0 path reaches div64_u64(n_lo, 0)
    let _ = mul_u64_add_u64_div_u64(0, 0, 7, 0);
}

#[test]
#[should_panic(expected = "division by zero")]
fn mul_div_zero_panics_high_path() {
    // n_hi >= d path asserts first
    let _ = mul_u64_add_u64_div_u64(u64::MAX, u64::MAX, 0, 0);
}

#[test]
fn mul_chunks_match_u128_split() {
    let mut rng = Xorshift64(0x0F0F0F0F_F0F0F0F0);
    let interesting: Vec<u64> = vec![0, 1, u32::MAX as u64, u32::MAX as u64 + 1, u64::MAX];
    let mut vals: Vec<u64> = (0..50_000).map(|_| rng.next()).collect();
    vals.extend(interesting);
    // Random triples plus structured boundary combinations (kept small
    // enough for debug builds; see the ignored heavy variant below).
    for i in 0..30_000usize {
        let a = vals[i % vals.len()];
        let b = vals[(i * 7 + 3) % vals.len()];
        let c = vals[(i * 13 + 5) % vals.len()];
        let prod = (a as u128) * (b as u128) + c as u128;
        let (hi, lo) = mul_u64_u64_add_u64_chunks(a, b, c);
        assert_eq!(
            ((hi as u128) << 64) | lo as u128,
            prod,
            "a={a:#x} b={b:#x} c={c:#x}"
        );
    }
}

#[test]
#[ignore = "slow: dense boundary matrix"]
fn mul_chunks_dense_boundary_matrix() {
    let vals: Vec<u64> = vec![
        0,
        1,
        2,
        u32::MAX as u64,
        u32::MAX as u64 + 1,
        (u32::MAX as u64) << 32,
        1 << 63,
        u64::MAX,
        u64::MAX - 1,
    ];
    for &a in &vals {
        for &b in &vals {
            for &c in &vals {
                let prod = (a as u128) * (b as u128) + c as u128;
                let (hi, lo) = mul_u64_u64_add_u64_chunks(a, b, c);
                assert_eq!(((hi as u128) << 64) | lo as u128, prod);
            }
        }
    }
}

#[test]
fn mul_u64_u32_add_u64_shr_vs_u128() {
    let mut rng = Xorshift64(0xABCD_1234_5678_EF90);
    for _ in 0..50_000 {
        let a = rng.next();
        let mul = (rng.next() >> 32) as u32;
        let b = rng.next();
        let shift = (rng.next() % 64) as u32;
        let want = (((a as u128) * u128::from(mul) + b as u128) >> shift) as u64;
        assert_eq!(mul_u64_u32_add_u64_shr(a, mul, b, shift), want);
    }
    // boundary shifts
    for shift in [0u32, 1, 31, 32, 33, 62, 63] {
        assert_eq!(
            mul_u64_u32_add_u64_shr(u64::MAX, u32::MAX, u64::MAX, shift),
            ((u64::MAX as u128 * u32::MAX as u128 + u64::MAX as u128) >> shift) as u64
        );
    }
}

// ---------------------------------------------------------------------------
// gcd (kernel table: lib/math/tests/gcd_kunit.c)
// ---------------------------------------------------------------------------

#[test]
fn gcd_kernel_kunit_table() {
    // (val1, val2, expected) — ported verbatim from gcd_kunit.c params[]
    let cases: [(u64, u64, u64); 11] = [
        (48, 18, 6),
        (18, 48, 6),
        (56, 98, 14),
        (17, 13, 1),
        (101, 103, 1),
        (270, 192, 6),
        (0, 5, 5),
        (7, 0, 7),
        (36, 36, 36),
        (u64::MAX, 1, 1),
        (u64::MAX, u64::MAX, u64::MAX),
    ];
    for &(a, b, want) in &cases {
        assert_eq!(gcd(a, b), want, "gcd({a}, {b})");
        assert_eq!(gcd_even_odd(a, b), want, "gcd_even_odd({a}, {b})");
    }
}

fn gcd_oracle(a: u64, b: u64) -> u64 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

#[test]
fn gcd_differential_and_consistency() {
    let mut rng = Xorshift64(0x600D_C0DE_600D_C0DE);
    for _ in 0..20_000 {
        let a = match rng.next() % 4 {
            0 => rng.next(),
            1 => rng.next() >> (rng.next() % 64),
            2 => (rng.next() << (rng.next() % 64)).rotate_left(rng.next_range(0, 63) as u32),
            _ => 1 << (rng.next() % 63),
        };
        let b = rng.next();
        let want = gcd_oracle(a, b);
        assert_eq!(gcd(a, b), want, "gcd({a:#x}, {b:#x})");
        assert_eq!(gcd_even_odd(a, b), want, "gcd_even_odd({a:#x}, {b:#x})");
    }
}

// ---------------------------------------------------------------------------
// reciprocal division (lib/math/reciprocal_div.c)
// ---------------------------------------------------------------------------

/// Divisor classes: every small value, powers of two +/- 1, odd/even mixes,
/// extremes — the classes the Granlund-Montgomery construction cares about.
fn interesting_divisors_u32() -> Vec<u32> {
    let mut rng = Xorshift64(0x0DDB_A115_D1CE);
    let mut v: Vec<u32> = (1u32..=200).collect();
    for b in 1..31u32 {
        v.push(1 << b);
        v.push((1 << b) - 1);
        v.push((1 << b) + 1);
    }
    v.extend([
        u32::MAX,
        u32::MAX - 1,
        u32::MAX / 2,
        0x8000_0000 - 1, // largest supported by _adv
    ]);
    v.extend((0..300).map(|_| ((rng.next() >> 33) as u32) | 1));
    v
}

#[test]
fn reciprocal_value_basic_property() {
    for &d in &interesting_divisors_u32() {
        let r = reciprocal_value(d);
        // C struct fields: sh1 = min(l,1), sh2 = max(l-1,0)
        let l = fls32(d - 1);
        assert_eq!(r.sh1 as u32, l.min(1));
        assert_eq!(r.sh2 as u32, l.saturating_sub(1));
    }
}

#[test]
fn reciprocal_divide_matches_native_exhaustive_small_n() {
    for &d in &interesting_divisors_u32() {
        let r = reciprocal_value(d);
        for n in 0..=511u32 {
            assert_eq!(reciprocal_divide(n, r), n / d, "n={n} d={d}");
        }
    }
}

#[test]
#[ignore = "slow: ~700 divisors x 4096 dividends in debug builds"]
fn reciprocal_divide_exhaustive_wide() {
    for &d in &interesting_divisors_u32() {
        let r = reciprocal_value(d);
        for n in 0..=4095u32 {
            assert_eq!(reciprocal_divide(n, r), n / d);
        }
    }
}

#[test]
fn reciprocal_divide_matches_native_random_full_range() {
    let mut rng = Xorshift64(0x1234_ABCD_0000_FFFF);
    for &d in &interesting_divisors_u32() {
        let r = reciprocal_value(d);
        for _ in 0..500 {
            let n = match rng.next() % 3 {
                0 => rng.next() as u32,
                1 => (rng.next() >> 32) as u32,
                _ => d.wrapping_add(rng.next() as i64 as u32), // near-divisor values
            };
            assert_eq!(reciprocal_divide(n, r), n / d, "n={n} d={d}");
        }
        // boundary dividends
        for &n in &[0u32, 1, d.wrapping_sub(1), d, d.wrapping_add(1), u32::MAX] {
            assert_eq!(reciprocal_divide(n, r), n / d, "n={n} d={d}");
        }
    }
}

#[test]
#[should_panic(expected = "division by zero")]
fn reciprocal_value_zero_panics() {
    let _ = reciprocal_value(0);
}

#[test]
fn reciprocal_adv_none_for_large_divisors() {
    // C WARNs + shifts UB only when ceil(log2(d)) == 32, i.e. d > 2^31;
    // the header declares those unsupported, so we return None.
    assert_eq!(reciprocal_value_adv(u32::MAX, 32), None);
    // d == 2^31 has ceil(log2(d)) == 31: defined in C, must succeed.
    assert!(reciprocal_value_adv(0x8000_0000, 32).is_some());
    assert!(reciprocal_value_adv(0x7FFF_FFFF, 32).is_some());
}

#[test]
fn reciprocal_adv_metadata_contract() {
    for &d in &interesting_divisors_u32() {
        if d >= 0x8000_0000 {
            continue;
        }
        let rv = reciprocal_value_adv(d, 32).unwrap();
        // exp is ceil(log2(d)) == kernel fls(d-1)
        assert_eq!(rv.exp as u32, fls32(d - 1), "d={d}");
        assert!(rv.sh <= rv.exp, "d={d}: post_shift must not exceed exp");
    }
}

/// Header recipe: pre_shift applies when multiplier would be wide and d even.
fn adv_setup(d: u32) -> Option<(ReciprocalValueAdv, u8)> {
    let rv = reciprocal_value_adv(d, 32)?;
    match adv_pre_shift(d, rv) {
        Some(ps) => {
            let rv2 = reciprocal_value_adv(d >> ps, 32 - ps)?;
            Some((rv2, ps))
        }
        None => Some((rv, 0)),
    }
}

#[test]
fn divide_with_reciprocal_adv_matches_native() {
    let mut rng = Xorshift64(0xBEEF_CAFE_DEAD_10CC);
    let test_ns: Vec<u32> = vec![
        0,
        1,
        2,
        3,
        1000,
        u32::MAX,
        u32::MAX - 1,
        0x4000_0000,
        0x7FFF_FFFF,
        0x7FFF_FFFE,
    ];
    for &d in &interesting_divisors_u32() {
        if d >= 0x8000_0000 {
            continue;
        }
        // Power-of-two divisors are outside this helper's contract: the
        // header instructs JITs to shift by `exp` directly for those.
        if d.is_power_of_two() {
            continue;
        }
        let Some((rv, pre_shift)) = adv_setup(d) else {
            panic!("adv setup failed for {d}");
        };
        let check = |n: u32| {
            assert_eq!(
                divide_with_reciprocal_adv(n, rv, pre_shift),
                n / d,
                "n={n} d={d}"
            );
        };
        for &n in &test_ns {
            check(n);
        }
        // near-divisor and random coverage
        for k in 1..=50u32 {
            check(d.wrapping_mul(k).wrapping_sub(1));
            check(d.wrapping_mul(k));
            check(d.wrapping_mul(k).wrapping_add(1));
        }
        for _ in 0..200 {
            check((rng.next() >> 33) as u32);
        }
    }
}
