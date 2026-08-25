//! Tests for the `lib/siphash.c` rewrite: exact vectors ported from
//! `lib/tests/siphash_kunit.c`, typed-vs-bytes endianness equivalence
//! checks, and a randomized differential comparison against an independent
//! transcription of the reference SipHash implementation (veorq/SipHash).

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;

use crate::*;
use alloc::vec::Vec;

/// Concatenate little-endian encodings into one message buffer.
fn msg(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// xorshift64 PRNG, self-contained (a shared krand crate lands later).
fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Test vectors taken from reference source available at:
/// <https://github.com/veorq/SipHash> (as in `lib/tests/siphash_kunit.c`).
const TEST_KEY_SIPHASH: SiphashKey = SiphashKey {
    key: [0x0706_0504_0302_0100, 0x0f0e_0d0c_0b0a_0908],
};

const TEST_VECTORS_SIPHASH: [u64; 64] = [
    0x726fdb47dd0e0e31,
    0x74f839c593dc67fd,
    0x0d6c8009d9a94f5a,
    0x85676696d7fb7e2d,
    0xcf2794e0277187b7,
    0x18765564cd99a68d,
    0xcbc9466e58fee3ce,
    0xab0200f58b01d137,
    0x93f5f5799a932462,
    0x9e0082df0ba9e4b0,
    0x7a5dbbc594ddb9f3,
    0xf4b32f46226bada7,
    0x751e8fbc860ee5fb,
    0x14ea5627c0843d90,
    0xf723ca908e7af2ee,
    0xa129ca6149be45e5,
    0x3f2acc7f57c29bdb,
    0x699ae9f52cbe4794,
    0x4bc1b3f0968dd39c,
    0xbb6dc91da77961bd,
    0xbed65cf21aa2ee98,
    0xd0f2cbb02e3b67c7,
    0x93536795e3a33e88,
    0xa80c038ccd5ccec8,
    0xb8ad50c6f649af94,
    0xbce192de8a85b8ea,
    0x17d835b85bbb15f3,
    0x2f2e6163076bcfad,
    0xde4daaaca71dc9a5,
    0xa6a2506687956571,
    0xad87a3535c49ef28,
    0x32d892fad841c342,
    0x7127512f72f27cce,
    0xa7f32346f95978e3,
    0x12e0b01abb051238,
    0x15e034d40fa197ae,
    0x314dffbe0815a3b4,
    0x027990f029623981,
    0xcadcd4e59ef40c4d,
    0x9abfd8766a33735c,
    0x0e3ea96b5304a7d0,
    0xad0c42d6fc585992,
    0x187306c89bc215a9,
    0xd4a60abcf3792b95,
    0xf935451de4f21df2,
    0xa9538f0419755787,
    0xdb9acddff56ca510,
    0xd06c98cd5c0975eb,
    0xe612a3cb9ecba951,
    0xc766e62cfcadaf96,
    0xee64435a9752fe72,
    0xa192d576b245165a,
    0x0a8787bf8ecb74b2,
    0x81b3e73d20b49b6f,
    0x7fa8220ba3b2ecea,
    0x245731c13ca42499,
    0xb78dbfaf3a8d83bd,
    0xea1ad565322a1a0b,
    0x60e61c23a3795013,
    0x6606d7e446282b93,
    0x6ca4ecb15c5f91e1,
    0x9f626da15c9625f3,
    0xe51b38608ef25f57,
    0x958a324ceb064572,
];

/// 64-bit-build HalfSipHash vectors (the kernel's KUnit uses these on
/// BITS_PER_LONG == 64).
const TEST_KEY_HSIPHASH: HsiphashKey = HsiphashKey {
    key: [0x0706_0504_0302_0100, 0x0f0e_0d0c_0b0a_0908],
};

const TEST_VECTORS_HSIPHASH: [u32; 64] = [
    0x050fc4dc, 0x7d57ca93, 0x4dc7d44d, 0xe7ddf7fb, 0x88d38328, 0x49533b67, 0xc59f22a7, 0x9bb11140,
    0x8d299a8e, 0x6c063de4, 0x92ff097f, 0xf94dc352, 0x57b4d9a2, 0x1229ffa7, 0xc0f95d34, 0x2a519956,
    0x7d908b66, 0x63dbd80c, 0xb473e63e, 0x8d297d1c, 0xa6cce040, 0x2b45f844, 0xa320872e, 0xdae6c123,
    0x67349c8c, 0x705b0979, 0xca9913a5, 0x4ade3b35, 0xef6cd00d, 0x4ab1e1f4, 0x43c5e663, 0x8c21d1bc,
    0x16a7b60d, 0x7a8ff9bf, 0x1f2a753e, 0xbf186b91, 0xada26206, 0xa3c33057, 0xae3a36a1, 0x7b108392,
    0x99e41531, 0x3f1ad944, 0xc8138825, 0xc28949a6, 0xfaf8876b, 0x9f042196, 0x68b1d623, 0x8b5114fd,
    0xdf074c46, 0x12cc86b3, 0x0a52098f, 0x9d292f9a, 0xa2f41f12, 0x43a71ed0, 0x73f0bce6, 0x70a7e980,
    0x243c6d75, 0xfdb71513, 0xa67d8a08, 0xb7e8f148, 0xf7a644ee, 0x0f1837f2, 0x4b6694e0, 0xb7bbb3a8,
];

/// Ported verbatim from `siphash_kunit.c`: sweep input lengths 1..=64 over
/// the byte-oriented entry points, then check every typed variant.
#[test]
fn kunit_siphash_vectors() {
    let mut input = [0u8; 64];
    for i in 0..64usize {
        input[i] = i as u8;
        assert_eq!(
            siphash(&input[..i], &TEST_KEY_SIPHASH),
            TEST_VECTORS_SIPHASH[i],
            "siphash aligned {}: FAIL",
            i + 1
        );
        // The C suite re-checks at an unaligned offset; in Rust there is one
        // canonical path, so the repeat is trivially equal but kept for
        // parity with the ported suite.
        assert_eq!(
            siphash(&input[..i], &TEST_KEY_SIPHASH),
            TEST_VECTORS_SIPHASH[i],
            "siphash unaligned {}: FAIL",
            i + 1
        );
        assert_eq!(
            hsiphash(&input[..i], &TEST_KEY_HSIPHASH),
            TEST_VECTORS_HSIPHASH[i],
            "hsiphash aligned {}: FAIL",
            i + 1
        );
        assert_eq!(
            hsiphash(&input[..i], &TEST_KEY_HSIPHASH),
            TEST_VECTORS_HSIPHASH[i],
            "hsiphash unaligned {}: FAIL",
            i + 1
        );
    }

    // Typed-variant checks straight out of the KUnit suite.
    assert_eq!(
        siphash_1u64(0x0706050403020100, &TEST_KEY_SIPHASH),
        TEST_VECTORS_SIPHASH[8]
    );
    assert_eq!(
        siphash_2u64(0x0706050403020100, 0x0f0e0d0c0b0a0908, &TEST_KEY_SIPHASH),
        TEST_VECTORS_SIPHASH[16]
    );
    assert_eq!(
        siphash_3u64(
            0x0706050403020100,
            0x0f0e0d0c0b0a0908,
            0x1716151413121110,
            &TEST_KEY_SIPHASH
        ),
        TEST_VECTORS_SIPHASH[24]
    );
    assert_eq!(
        siphash_4u64(
            0x0706050403020100,
            0x0f0e0d0c0b0a0908,
            0x1716151413121110,
            0x1f1e1d1c1b1a1918,
            &TEST_KEY_SIPHASH
        ),
        TEST_VECTORS_SIPHASH[32]
    );
    assert_eq!(
        siphash_1u32(0x03020100, &TEST_KEY_SIPHASH),
        TEST_VECTORS_SIPHASH[4]
    );
    assert_eq!(
        siphash_2u32(0x03020100, 0x07060504, &TEST_KEY_SIPHASH),
        TEST_VECTORS_SIPHASH[8]
    );
    assert_eq!(
        siphash_3u32(0x03020100, 0x07060504, 0x0b0a0908, &TEST_KEY_SIPHASH),
        TEST_VECTORS_SIPHASH[12]
    );
    assert_eq!(
        siphash_4u32(
            0x03020100,
            0x07060504,
            0x0b0a0908,
            0x0f0e0d0c,
            &TEST_KEY_SIPHASH
        ),
        TEST_VECTORS_SIPHASH[16]
    );
    assert_eq!(
        hsiphash_1u32(0x03020100, &TEST_KEY_HSIPHASH),
        TEST_VECTORS_HSIPHASH[4]
    );
    assert_eq!(
        hsiphash_2u32(0x03020100, 0x07060504, &TEST_KEY_HSIPHASH),
        TEST_VECTORS_HSIPHASH[8]
    );
    assert_eq!(
        hsiphash_3u32(0x03020100, 0x07060504, 0x0b0a0908, &TEST_KEY_HSIPHASH),
        TEST_VECTORS_HSIPHASH[12]
    );
    assert_eq!(
        hsiphash_4u32(
            0x03020100,
            0x07060504,
            0x0b0a0908,
            0x0f0e0d0c,
            &TEST_KEY_HSIPHASH
        ),
        TEST_VECTORS_HSIPHASH[16]
    );
}

/// Endianness contract: typed inputs mix into the state numerically, byte
/// buffers decode little-endian — so hashing `x.to_le_bytes()` must equal
/// the typed call for every width, on random data.
#[test]
fn typed_matches_le_bytes() {
    let mut s = 0x0123_4567_89ab_cdef_u64;
    let key = SiphashKey {
        key: [next(&mut s), next(&mut s)],
    };
    let hkey = HsiphashKey {
        key: [next(&mut s), next(&mut s)],
    };
    for _ in 0..256 {
        let w1 = next(&mut s);
        let w2 = next(&mut s);
        let w3 = next(&mut s);
        let w4 = next(&mut s);
        let _ = (w3, w4); // extra entropy consumed to vary sequences
        let (a, b, c, d) = (w1 as u32, (w1 >> 32) as u32, w2 as u32, (w2 >> 32) as u32);

        assert_eq!(siphash_1u64(w1, &key), siphash(&w1.to_le_bytes(), &key));
        assert_eq!(
            siphash_2u64(w1, w2, &key),
            siphash(&[w1.to_le_bytes(), w2.to_le_bytes()].concat(), &key)
        );
        assert_eq!(siphash_1u32(a, &key), siphash(&a.to_le_bytes(), &key));
        assert_eq!(
            siphash_2u32(a, b, &key),
            siphash(&msg(&[&(a as u64 | (b as u64) << 32).to_le_bytes()]), &key)
        );
        assert_eq!(
            siphash_3u32(a, b, c, &key),
            siphash(
                &msg(&[
                    &(a as u64 | (b as u64) << 32).to_le_bytes(),
                    &c.to_le_bytes()
                ]),
                &key
            )
        );
        assert_eq!(
            siphash_4u32(a, b, c, d, &key),
            siphash(
                &msg(&[
                    &(a as u64 | (b as u64) << 32).to_le_bytes(),
                    &(c as u64 | (d as u64) << 32).to_le_bytes()
                ]),
                &key,
            )
        );

        assert_eq!(hsiphash_1u32(a, &hkey), hsiphash(&a.to_le_bytes(), &hkey));
        assert_eq!(
            hsiphash_2u32(a, b, &hkey),
            hsiphash(&msg(&[&(a as u64 | (b as u64) << 32).to_le_bytes()]), &hkey)
        );
        assert_eq!(
            hsiphash_3u32(a, b, c, &hkey),
            hsiphash(
                &msg(&[
                    &(a as u64 | (b as u64) << 32).to_le_bytes(),
                    &c.to_le_bytes()
                ]),
                &hkey
            )
        );
        assert_eq!(
            hsiphash_4u32(a, b, c, d, &hkey),
            hsiphash(
                &msg(&[
                    &(a as u64 | (b as u64) << 32).to_le_bytes(),
                    &(c as u64 | (d as u64) << 32).to_le_bytes()
                ]),
                &hkey,
            )
        );
    }
}

// ---------------------------------------------------------------------------
// Independent reference implementation (transcribed from the SipHash paper /
// veorq/SipHash reference), used as a differential oracle. Written in the
// classic state-array style, deliberately structured differently from lib.rs.
// ---------------------------------------------------------------------------

fn ref_rotl(x: u64, b: u32) -> u64 {
    x.rotate_left(b)
}

fn ref_round(v: &mut [u64; 4]) {
    v[0] = v[0].wrapping_add(v[1]);
    v[1] = ref_rotl(v[1], 13);
    v[1] ^= v[0];
    v[0] = ref_rotl(v[0], 32);
    v[2] = v[2].wrapping_add(v[3]);
    v[3] = ref_rotl(v[3], 16);
    v[3] ^= v[2];
    v[0] = v[0].wrapping_add(v[3]);
    v[3] = ref_rotl(v[3], 21);
    v[3] ^= v[0];
    v[2] = v[2].wrapping_add(v[1]);
    v[1] = ref_rotl(v[1], 17);
    v[1] ^= v[2];
    v[2] = ref_rotl(v[2], 32);
}

/// Reference SipHash with configurable round counts (c compression rounds,
/// d finalization rounds); `(2, 4)` is SipHash-2-4, `(1, 3)` is the 64-bit
/// HalfSipHash substitution.
fn ref_siphash(data: &[u8], k0: u64, k1: u64, c: u32, d: u32) -> u64 {
    let mut v: [u64; 4] = [
        0x736f_6d65_7073_6575 ^ k0,
        0x646f_7261_6e64_6f6d ^ k1,
        0x6c79_6765_6e65_7261 ^ k0,
        0x7465_6462_7974_6573 ^ k1,
    ];

    let mut i = 0usize;
    while i + 8 <= data.len() {
        let mut m = [0u8; 8];
        m.copy_from_slice(&data[i..i + 8]);
        v[3] ^= u64::from_le_bytes(m);
        for _ in 0..c {
            ref_round(&mut v);
        }
        v[0] ^= u64::from_le_bytes(m);
        i += 8;
    }

    // Final block: length byte plus little-endian tail.
    let mut b: u64 = ((data.len() as u64) & 0xff) << 56;
    for (j, &byte) in data[i..].iter().enumerate() {
        b |= (byte as u64) << (8 * j);
    }
    v[3] ^= b;
    for _ in 0..c {
        ref_round(&mut v);
    }
    v[0] ^= b;
    v[2] ^= 0xff;
    for _ in 0..d {
        ref_round(&mut v);
    }
    v[0] ^ v[1] ^ v[2] ^ v[3]
}

/// Differential fuzz against the reference implementation: random keys,
/// random messages of many lengths (all tails exercised), both algorithms.
#[test]
fn differential_vs_reference() {
    let mut s = 0xa5a5_5a5a_dead_10cc_u64;

    for len in 0..=200usize {
        let msg: Vec<u8> = (0..len).map(|_| next(&mut s) as u8).collect();

        // Fresh keys per iteration to widen coverage.
        let key = SiphashKey {
            key: [next(&mut s), next(&mut s)],
        };
        let hkey = HsiphashKey {
            key: [next(&mut s), next(&mut s)],
        };

        assert_eq!(
            siphash(&msg, &key),
            ref_siphash(&msg, key.key[0], key.key[1], 2, 4),
            "siphash mismatch at len {len}"
        );
        assert_eq!(
            hsiphash(&msg, &hkey) as u64,
            ref_siphash(&msg, hkey.key[0], hkey.key[1], 1, 3) & 0xffff_ffff,
            "hsiphash mismatch at len {len}"
        );
    }
}

/// `siphash_key_is_zero()` parity.
#[test]
fn key_is_zero() {
    assert!(SiphashKey::default().is_zero());
    assert!(!TEST_KEY_SIPHASH.is_zero());
    let one = SiphashKey { key: [0, 1] };
    assert!(!one.is_zero());
}

/// Length mixing is mod 256 (C shifts `len` into the top byte of a u64);
/// messages of length L and L+256 differ only by that byte being aliased —
/// verify our behavior matches the reference exactly at such lengths.
#[test]
fn long_message_length_aliasing() {
    let mut s = 0x600d_cafe_f00d_face_u64;
    let key = SiphashKey {
        key: [next(&mut s), next(&mut s)],
    };
    for len in [255usize, 256, 257, 511, 512] {
        let msg: Vec<u8> = (0..len).map(|_| next(&mut s) as u8).collect();
        assert_eq!(
            siphash(&msg, &key),
            ref_siphash(&msg, key.key[0], key.key[1], 2, 4),
            "len {len}"
        );
    }
}
