//! Tests for the `lib/crc/crc32-main.c` rewrite: the randomized differential
//! scheme from `lib/crc/tests/crc_kunit.c`, canonical known-answer vectors,
//! table spot-values pinned to the C generator's output, and slice-by-8 vs
//! base equivalence fuzzing.

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;

use alloc::{vec, vec::Vec};

use super::*;

/// Self-contained deterministic PRNG (xorshift64) so this crate has no deps.
struct Xorshift64(u64);

impl Xorshift64 {
    fn next_u32(&mut self) -> u32 {
        // Split into two 32-bit draws like prandom_u32_state usage patterns.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    fn next_u64(&mut self) -> u64 {
        let n = self.next_u32() as u64;
        (n << 32) | self.next_u32() as u64
    }
}

/// Port of `struct crc_variant` from lib/crc/tests/crc_kunit.c (the 32-bit
/// variants relevant to crc32-main.c).
struct Variant {
    le: bool,
    poly: u32,
}

impl Variant {
    const fn mask(&self) -> u32 {
        u32::MAX // all variants here are 32 bits
    }
}

/// Port of `crc_ref()` from lib/crc/tests/crc_kunit.c: reference bit-at-a-time
/// implementation of any CRC variant (no initial/final inversion).
fn crc_ref(v: &Variant, mut crc: u32, p: &[u8]) -> u32 {
    for &b in p {
        for j in 0..8 {
            if v.le {
                crc ^= ((b >> j) & 1) as u32;
                crc = (crc >> 1) ^ if crc & 1 != 0 { v.poly } else { 0 };
            } else {
                crc ^= (((b >> (7 - j)) & 1) as u32) << 31;
                if crc & (1 << 31) != 0 {
                    crc = (crc << 1) ^ v.poly;
                } else {
                    crc <<= 1;
                }
            }
        }
    }
    crc
}

/// The three variants exercised by the kernel test for these functions.
const VARIANTS: [(&str, Variant); 3] = [
    ("crc32_le", Variant { le: true, poly: CRC32_POLY_LE }),
    ("crc32_be", Variant { le: false, poly: CRC32_POLY_BE }),
    ("crc32c", Variant { le: true, poly: CRC32C_POLY_LE }),
];

fn apply(v: &Variant, crc: u32, data: &[u8]) -> u32 {
    match v.poly {
        p if p == CRC32_POLY_LE && v.le => crc32_le(crc, data),
        p if p == CRC32_POLY_BE && !v.le => crc32_be(crc, data),
        _ => crc32c(crc, data),
    }
}

/// Port of `generate_random_initial_crc()` from lib/crc/tests/crc_kunit.c.
fn generate_random_initial_crc(rng: &mut Xorshift64, v: &Variant) -> u32 {
    match rng.next_u32() % 4 {
        0 => 0,
        1 => v.mask(), // all 1 bits
        _ => rng.next_u64() as u32 & v.mask(),
    }
}

/// Port of `generate_random_length()` from lib/crc/tests/crc_kunit.c:
/// prefers small lengths.
fn generate_random_length(rng: &mut Xorshift64, max_length: usize) -> usize {
    let len: usize = match rng.next_u32() % 3 {
        0 => (rng.next_u32() % 128) as usize,
        1 => (rng.next_u32() % 3072) as usize,
        _ => rng.next_u64() as usize,
    };
    len % (max_length + 1)
}

/// Port of the core `crc_test()` loop from lib/crc/tests/crc_kunit.c
/// (CRC_KUNIT_SEED=42, CRC_KUNIT_NUM_TEST_ITERS=1000, MAX_LEN=16384): the
/// table-driven implementations must agree with the bitwise reference on
/// randomly generated inputs, lengths, alignments and initial CRCs.
#[test]
fn differential_vs_bitwise_reference() {
    const SEED: u64 = 42;
    const NUM_ITERS: usize = 1000;
    const MAX_LEN: usize = 16384;

    let mut rng = Xorshift64(SEED);
    let mut buf = vec![0u8; MAX_LEN];
    for b in buf.iter_mut() {
        *b = rng.next_u32() as u8; // prandom_bytes_state equivalent
    }

    for (name, v) in VARIANTS.iter() {
        let mut rng = Xorshift64(SEED.wrapping_add(name.len() as u64));
        for i in 0..NUM_ITERS {
            let init_crc = generate_random_initial_crc(&mut rng, v);
            let len = generate_random_length(&mut rng, MAX_LEN);
            let offset = if rng.next_u32() % 2 == 0 {
                // Random alignment mod 64.
                (rng.next_u32() as usize % 64).min(MAX_LEN - len)
            } else {
                // Tail position (C uses the guard-page end).
                MAX_LEN - len
            };

            if rng.next_u32() % 8 == 0 {
                // Refresh the data occasionally.
                for b in &mut buf[offset..offset + len] {
                    *b = rng.next_u32() as u8;
                }
            }

            let expected = crc_ref(v, init_crc, &buf[offset..offset + len]);
            let actual = apply(v, init_crc, &buf[offset..offset + len]);
            assert_eq!(
                actual, expected,
                "{name}: wrong result with len={len} offset={offset} iter={i}"
            );
        }
    }
}

/// Known-answer vectors. Canonical values are the standard "123456789" checks
/// with init=!0 and final xor=!0 applied by the *caller*; raw expectations are
/// what crc32_le()/crc32c() must return before that final inversion.
/// Verified by independent Python transcription of the reflected algorithm.
#[test]
fn canonical_vectors() {
    let d = b"123456789";

    // IEEE CRC-32: canonical result 0xcbf43926 after xor-ing with !0.
    assert_eq!(crc32_le(!0u32, d), 0x340b_c6d9);
    assert_eq!(crc32_le(!0u32, d) ^ !0u32, 0xcbf4_3926);
    // CRC-32C: canonical result 0xe3069283 after xor-ing with !0.
    assert_eq!(crc32c(!0u32, d), 0x1cf9_6d7c);
    assert_eq!(crc32c(!0u32, d) ^ !0u32, 0xe306_9283);

    // No inversion happens inside the functions: empty input is identity.
    assert_eq!(crc32_le(0x1234_5678, b""), 0x1234_5678);
    assert_eq!(crc32_le(0, b""), 0);
    assert_eq!(crc32_be(0xdeadbeef, b""), 0xdead_beef);
    assert_eq!(crc32c(0, b""), 0);

    // Single bytes against hand-computed table lookups.
    assert_eq!(crc32_le(0, &[0x00]), 0); // t[0 ^ 0]
    assert_eq!(crc32_le(0, &[0xff]), CRC32TABLE_LE[0xff]);

    // The crc32() alias matches crc32_le().
    assert_eq!(crc32(!0u32, d), crc32_le(!0u32, d));

    // Incremental composition (affinity in state), like calling crc32_le()
    // per-packet and chaining the result:
    let (a, b) = (&d[..4], &d[4..]);
    let whole = crc32_le(!0u32, d);
    let split = crc32_le(crc32_le(!0u32, a), b);
    assert_eq!(whole, split);
    let whole_c = crc32c(!0u32, d);
    let split_c = crc32c(crc32c(!0u32, a), b);
    assert_eq!(whole_c, split_c);
}

/// Table spot values pinned against output transcribed from
/// lib/crc/gen_crc32table.c logic (verified independently in Python).
#[test]
fn tables_match_c_generator() {
    // crc32table_le
    assert_eq!(CRC32TABLE_LE[0], 0x0000_0000);
    assert_eq!(CRC32TABLE_LE[1], 0x7707_3096);
    assert_eq!(CRC32TABLE_LE[128], CRC32_POLY_LE); // t[128] == poly
    assert_eq!(CRC32TABLE_LE[255], 0x2d02_ef8d);
    // crc32table_be
    assert_eq!(CRC32TABLE_BE[0], 0x0000_0000);
    assert_eq!(CRC32TABLE_BE[1], 0x04c1_1db7);
    assert_eq!(CRC32TABLE_BE[128], 0x690c_e0ee);
    assert_eq!(CRC32TABLE_BE[255], 0xb1f7_40b4);
    // crc32ctable_le
    assert_eq!(CRC32CTABLE_LE[0], 0x0000_0000);
    assert_eq!(CRC32CTABLE_LE[1], 0xf26b_8303);
    assert_eq!(CRC32CTABLE_LE[128], CRC32C_POLY_LE); // t[128] == poly
    assert_eq!(CRC32CTABLE_LE[255], 0xad7d_5351);

    // Structural property from gen_crc32table.c: tab[i^j] = tab[i] ^ tab[j]
    // holds for the linear table construction (spot-check 64 random pairs).
    let mut s = 0x5eed_c32f_u64;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for _ in 0..64 {
        let i = (next() % 256) as usize;
        let j = (next() % 256) as usize;
        assert_eq!(
            CRC32TABLE_LE[i ^ j],
            CRC32TABLE_LE[i] ^ CRC32TABLE_LE[j],
            "linearity broken at ({i},{j})"
        );
        assert_eq!(
            CRC32CTABLE_LE[i ^ j],
            CRC32CTABLE_LE[i] ^ CRC32CTABLE_LE[j],
            "linearity broken at ({i},{j}) for crc32c"
        );
    }
}

/// Slicing-by-8 must produce results bit-exact with the byte-at-a-time base
/// implementations across random inputs, chunk boundaries and seeds.
#[test]
fn slice8_equals_base() {
    let mut s = 0x1234_5678_9abc_def0_u64;
    let mut next32 = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 32) as u32
    };

    for case in 0..2000usize {
        let len = (next32() % 80) as usize; // straddles the 8-byte boundary often
        let data: Vec<u8> = (0..len).map(|_| next32() as u8).collect();
        let init = next32();

        assert_eq!(
            crc32_le(init, &data),
            crc32_le_base(init, &data),
            "crc32_le mismatch, case {case}, len {len}"
        );
        assert_eq!(
            crc32c(init, &data),
            crc32c_base(init, &data),
            "crc32c mismatch, case {case}, len {len}"
        );
    }

    // Exact-boundary cases around multiples of 8.
    for len in [7usize, 8, 9, 15, 16, 17] {
        let data = vec![0xa5u8; len];
        assert_eq!(crc32_le(0xffff_ffff, &data), crc32_le_base(0xffff_ffff, &data));
        assert_eq!(crc32c(0xffff_ffff, &data), crc32c_base(0xffff_ffff, &data));
    }
}

/// All-zero and all-one inputs must match the bitwise reference exactly —
/// cheap exhaustive-ish corner coverage beyond the randomized suite.
#[test]
fn degenerate_inputs_match_reference() {
    let le_v = Variant { le: true, poly: CRC32_POLY_LE };
    let be_v = Variant { le: false, poly: CRC32_POLY_BE };
    let c_v = Variant { le: true, poly: CRC32C_POLY_LE };

    for len in [1usize, 2, 7, 8, 9, 16, 63, 64, 65] {
        for fill in [0x00u8, 0xff] {
            let data = vec![fill; len];
            assert_eq!(crc32_le(0, &data), crc_ref(&le_v, 0, &data));
            assert_eq!(crc32_le(!0, &data), crc_ref(&le_v, !0, &data));
            assert_eq!(crc32_be(0, &data), crc_ref(&be_v, 0, &data));
            assert_eq!(crc32_be(!0, &data), crc_ref(&be_v, !0, &data));
            assert_eq!(crc32c(0, &data), crc_ref(&c_v, 0, &data));
            assert_eq!(crc32c(!0, &data), crc_ref(&c_v, !0, &data));
        }
    }
}

/// Regression pin for the slice-by-8 tail bug (tail bytes were once routed
/// through the IEEE table for every polynomial): 9-byte input forces one
/// chunk plus exactly one tail byte.
#[test]
fn slice_tail_uses_same_polynomial() {
    let v = Variant { le: true, poly: CRC32C_POLY_LE };
    assert_eq!(crc32c_base(!0, &[0x31u8]), crc_ref(&v, !0, &[0x31]), "1 byte");
    assert_eq!(crc32c_base(!0, b"12345678"), crc_ref(&v, !0, b"12345678"), "8 bytes");
    assert_eq!(crc32c(!0, b"123456789"), crc_ref(&v, !0, b"123456789"), "slice+tail 9");
    assert_eq!(crc32c_base(!0, b"123456789"), crc_ref(&v, !0, b"123456789"), "base 9");
}
