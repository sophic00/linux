// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's `lib/xxhash.c` — the xxHash extremely
//! fast hash algorithm (one-shot `xxh32()` / `xxh64()`).
//!
//! Copyright (C) 2012-2016, Yann Collet. Dual BSD/GPL licensed upstream;
//! rewritten here under GPL-2.0 like the rest of this workspace.
//!
//! Faithfulness notes / deviations from C:
//! - The C API takes `(ptr, len, seed)`; here `input: &[u8]` carries the
//!   length, so there is no separate `len` parameter and no NULL check.
//! - `get_unaligned_le32/64` become plain little-endian byte loads
//!   (`from_le_bytes`), which are unaligned-safe by definition in Rust.
//! - All arithmetic is explicitly wrapping, matching C unsigned wraparound.
//!
//! Test vectors below are the canonical xxHash reference values, confirmed
//! against the authoritative `xxhash` python reference package, and the
//! tests additionally include a naive independently written reference
//! implementation that sweeps every input length to exercise all tail paths.

#![no_std]
#![deny(unsafe_code)]

/// `PRIME32_1`.
const PRIME32_1: u32 = 2654435761;
/// `PRIME32_2`.
const PRIME32_2: u32 = 2246822519;
/// `PRIME32_3`.
const PRIME32_3: u32 = 3266489917;
/// `PRIME32_4`.
const PRIME32_4: u32 = 668265263;
/// `PRIME32_5`.
const PRIME32_5: u32 = 374761393;

/// `PRIME64_1`.
const PRIME64_1: u64 = 11400714785074694791;
/// `PRIME64_2`.
const PRIME64_2: u64 = 14029467366897019727;
/// `PRIME64_3`.
const PRIME64_3: u64 = 1609587929392839161;
/// `PRIME64_4`.
const PRIME64_4: u64 = 9650029242287828579;
/// `PRIME64_5`.
const PRIME64_5: u64 = 2870177450012600261;

#[inline]
fn read_le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

#[inline]
fn read_le64(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// `xxh32_round()`.
fn xxh32_round(seed: u32, input: u32) -> u32 {
    let acc = seed.wrapping_add(input.wrapping_mul(PRIME32_2));
    acc.rotate_left(13).wrapping_mul(PRIME32_1)
}

/// `xxh32()`: calculate the xxHash of the input (32-bit variant).
///
/// Equivalent to the kernel's `xxh32(input, len, seed)` with `len` taken
/// from `input.len()`.
pub fn xxh32(input: &[u8], seed: u32) -> u32 {
    let len = input.len();
    let mut p = input;

    // The C code uses a do-while that processes one 16-byte block then
    // continues while `p <= b_end - 16`, which is exactly "while at least
    // one more full 16-byte block remains".
    let mut h32;
    if len >= 16 {
        let mut v1 = seed.wrapping_add(PRIME32_1).wrapping_add(PRIME32_2);
        let mut v2 = seed.wrapping_add(PRIME32_2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(PRIME32_1);

        while p.len() >= 16 {
            v1 = xxh32_round(v1, read_le32(p));
            v2 = xxh32_round(v2, read_le32(&p[4..]));
            v3 = xxh32_round(v3, read_le32(&p[8..]));
            v4 = xxh32_round(v4, read_le32(&p[12..]));
            p = &p[16..];
        }

        h32 = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
    } else {
        h32 = seed.wrapping_add(PRIME32_5);
    }

    h32 = h32.wrapping_add(len as u32);

    while p.len() >= 4 {
        h32 = h32
            .wrapping_add(read_le32(p).wrapping_mul(PRIME32_3))
            .rotate_left(17)
            .wrapping_mul(PRIME32_4);
        p = &p[4..];
    }

    for &b in p {
        h32 = h32
            .wrapping_add((b as u32).wrapping_mul(PRIME32_5))
            .rotate_left(11)
            .wrapping_mul(PRIME32_1);
    }

    h32 ^= h32 >> 15;
    h32 = h32.wrapping_mul(PRIME32_2);
    h32 ^= h32 >> 13;
    h32 = h32.wrapping_mul(PRIME32_3);
    h32 ^= h32 >> 16;

    h32
}

/// `xxh64_round()`.
fn xxh64_round(acc: u64, input: u64) -> u64 {
    let acc = acc.wrapping_add(input.wrapping_mul(PRIME64_2));
    acc.rotate_left(31).wrapping_mul(PRIME64_1)
}

/// `xxh64_merge_round()`.
fn xxh64_merge_round(mut acc: u64, val: u64) -> u64 {
    let val = xxh64_round(0, val);
    acc ^= val;
    acc.wrapping_mul(PRIME64_1).wrapping_add(PRIME64_4)
}

/// `xxh64()`: calculate the xxHash of the input (64-bit variant).
///
/// Equivalent to the kernel's `xxh64(input, len, seed)` with `len` taken
/// from `input.len()`. (The streaming `xxh64_reset/update/digest` state
/// machine is not part of this rewrite; it is scheduling-adjacent stateful
/// API surface and can be layered on top if ever needed.)
pub fn xxh64(input: &[u8], seed: u64) -> u64 {
    let len = input.len();
    let mut p = input;

    let mut h64 = if len >= 32 {
        let mut v1 = seed.wrapping_add(PRIME64_1).wrapping_add(PRIME64_2);
        let mut v2 = seed.wrapping_add(PRIME64_2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(PRIME64_1);

        while p.len() >= 32 {
            v1 = xxh64_round(v1, read_le64(p));
            v2 = xxh64_round(v2, read_le64(&p[8..]));
            v3 = xxh64_round(v3, read_le64(&p[16..]));
            v4 = xxh64_round(v4, read_le64(&p[24..]));
            p = &p[32..];
        }

        let mut h = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
        h = xxh64_merge_round(h, v1);
        h = xxh64_merge_round(h, v2);
        h = xxh64_merge_round(h, v3);
        h = xxh64_merge_round(h, v4);
        h
    } else {
        seed.wrapping_add(PRIME64_5)
    };

    h64 = h64.wrapping_add(len as u64);

    while p.len() >= 8 {
        let k1 = xxh64_round(0, read_le64(p));
        h64 ^= k1;
        h64 = h64.rotate_left(27).wrapping_mul(PRIME64_1).wrapping_add(PRIME64_4);
        p = &p[8..];
    }

    if p.len() >= 4 {
        h64 ^= (read_le32(p) as u64).wrapping_mul(PRIME64_1);
        h64 = h64.rotate_left(23).wrapping_mul(PRIME64_2).wrapping_add(PRIME64_3);
        p = &p[4..];
    }

    for &b in p {
        h64 ^= (b as u64).wrapping_mul(PRIME64_5);
        h64 = h64.rotate_left(11).wrapping_mul(PRIME64_1);
    }

    h64 ^= h64 >> 33;
    h64 = h64.wrapping_mul(PRIME64_2);
    h64 ^= h64 >> 29;
    h64 = h64.wrapping_mul(PRIME64_3);
    h64 ^= h64 >> 32;

    h64
}

#[cfg(test)]
mod tests;
