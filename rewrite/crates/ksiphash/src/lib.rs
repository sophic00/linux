// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's `lib/siphash.c` — SipHash: a fast
//! short-input PRF (https://131002.net/siphash/).
//!
//! Upstream is (GPL-2.0-only OR BSD-3-Clause), Copyright (C) 2016-2022 Jason
//! A. Donenfeld; rewritten here under GPL-2.0 like the rest of this workspace.
//!
//! This implementation is specifically SipHash-2-4 for a secure PRF and
//! HalfSipHash-1-3 for an insecure hashtable-only PRF.
//!
//! # C-to-Rust correspondence
//!
//! | C symbol (`lib/siphash.c` / `include/linux/siphash.h`) | Rust symbol | Notes |
//! |---|---|---|
//! | `SIPHASH_PERMUTATION` / `SIPROUND` | [`sip_round`] | |
//! | `SIPHASH_CONST_0..3` | [`SIPHASH_CONST_0`]..[`SIPHASH_CONST_3`] | |
//! | `siphash_key_t`, `siphash_key_is_zero()` | [`SiphashKey`], [`SiphashKey::is_zero`] | |
//! | `__siphash_aligned()` / `__siphash_unaligned()` / `siphash()` | [`siphash`] | see deviation D1 |
//! | `siphash_1u64()` .. `siphash_4u64()` | [`siphash_1u64`] .. [`siphash_4u64`] | |
//! | `siphash_1u32()`, `siphash_3u32()` | [`siphash_1u32`], [`siphash_3u32`] | |
//! | `siphash_2u32()`, `siphash_4u32()` (header inlines) | [`siphash_2u32`], [`siphash_4u32`] | same word-packing as C |
//! | `HSIPROUND`, 64-bit `HPREAMBLE`/`HPOSTAMBLE` | 64-bit `HSIPROUND`/`HPOSTAMBLE` macros | inlined into [`hsiphash`] and the typed fns | HSIPROUND == SIPROUND on 64-bit |
//! | `__hsiphash_aligned()` / `__hsiphash_unaligned()` / `hsiphash()` (64-bit) | [`hsiphash`] | see deviations D1, D2 |
//! | `hsiphash_1u32()` .. `hsiphash_4u32()` (64-bit) | [`hsiphash_1u32`] .. [`hsiphash_4u32`] | |
//!
//! # Documented deviations
//!
//! * **D1 — one canonical path.** The C split into `__aligned` /
//!   `__unaligned` variants exists purely to let aligned builds do wide
//!   loads; both produce identical hashes. In Rust a byte slice load is
//!   unaligned-safe by construction, so a single path serves both. The C
//!   `load_unaligned_zeropad()` fault-fixup trick (reading past the buffer
//!   end within a page) is replaced by reading only the bytes that exist;
//!   the observable hash values are identical.
//! * **D2 — 64-bit build configuration.** On `BITS_PER_LONG == 64` the
//!   kernel implements HalfSipHash as SipHash-1-3 with 64-bit state (a
//!   documented performance substitution); this crate implements exactly
//!   that configuration, matching the kernel's own KUnit vectors for 64-bit
//!   builds (`lib/tests/siphash_kunit.c`). The true 32-bit-state
//!   HalfSipHash from the `#else` branch of `lib/siphash.c` produces
//!   different values and is intentionally not implemented here.
//! * All arithmetic uses explicit wrapping operations, matching C unsigned
//!   wraparound. Rotations use [`u64::rotate_left`].
//! * Typed-input functions (`*_1u64`, `*_3u32`, ...) mix caller-supplied
//!   integer values directly into the state with no endianness conversion,
//!   exactly like the C macros; byte-buffer functions interpret input as
//!   little-endian words, exactly like `get_unaligned_le*`.

#![no_std]
#![deny(unsafe_code)]

/// `SIPHASH_CONST_0`: "somepseu".
pub const SIPHASH_CONST_0: u64 = 0x736f_6d65_7073_6575;
/// `SIPHASH_CONST_1`: "dorandom".
pub const SIPHASH_CONST_1: u64 = 0x646f_7261_6e64_6f6d;
/// `SIPHASH_CONST_2`: "lygener".
pub const SIPHASH_CONST_2: u64 = 0x6c79_6765_6e65_7261;
/// `SIPHASH_CONST_3`: "datedtes".
pub const SIPHASH_CONST_3: u64 = 0x7465_6462_7974_6573;

/// `siphash_key_t`: a 128-bit SipHash key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(align(8))]
pub struct SiphashKey {
    /// Raw key words, mixed into the state as-is (see module docs).
    pub key: [u64; 2],
}

impl SiphashKey {
    /// `siphash_key_is_zero()`.
    pub fn is_zero(&self) -> bool {
        // C: `return !(key->key[0] | key->key[1]);` where `!` is *logical*
        // negation; translating it as Rust's bitwise `!` would invert it.
        (self.key[0] | self.key[1]) == 0
    }
}

/// `hsiphash_key_t` on 64-bit builds (`unsigned long` == `u64`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(align(8))]
pub struct HsiphashKey {
    /// Raw key words.
    pub key: [u64; 2],
}

/// One SipHash round (`SIPROUND`): the SipHash permutation on four words.
#[inline(always)]
fn sip_round(v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64) {
    *v0 = v0.wrapping_add(*v1);
    *v1 = v1.rotate_left(13);
    *v1 ^= *v0;
    *v0 = v0.rotate_left(32);
    *v2 = v2.wrapping_add(*v3);
    *v3 = v3.rotate_left(16);
    *v3 ^= *v2;
    *v0 = v0.wrapping_add(*v3);
    *v3 = v3.rotate_left(21);
    *v3 ^= *v0;
    *v2 = v2.wrapping_add(*v1);
    *v1 = v1.rotate_left(17);
    *v1 ^= *v2;
    *v2 = v2.rotate_left(32);
}

/// Little-endian tail packing: the C switch ORs each remaining byte at its
/// position; equivalently byte `i` contributes `byte << (8*i)`.
#[inline]
fn tail_le(data: &[u8]) -> u64 {
    data.iter()
        .enumerate()
        .fold(0u64, |acc, (i, &b)| acc | (b as u64) << (8 * i))
}

/// State after `PREAMBLE(len)` plus the full-block compression loop shared
/// by the byte-oriented entry points.
struct SipState {
    v0: u64,
    v1: u64,
    v2: u64,
    v3: u64,
    b: u64,
}

impl SipState {
    /// `PREAMBLE(len)`: initialize state from key and message length.
    #[inline]
    fn new(key: &[u64; 2], len: usize) -> Self {
        let mut s = SipState {
            v0: SIPHASH_CONST_0,
            v1: SIPHASH_CONST_1,
            v2: SIPHASH_CONST_2,
            v3: SIPHASH_CONST_3,
            b: (len as u64) << 56,
        };
        s.v3 ^= key[1];
        s.v2 ^= key[0];
        s.v1 ^= key[1];
        s.v0 ^= key[0];
        s
    }

    /// Compress one full little-endian 8-byte block.
    #[inline]
    fn block(&mut self, m: u64, c: u32) {
        self.v3 ^= m;
        for _ in 0..c {
            sip_round(&mut self.v0, &mut self.v1, &mut self.v2, &mut self.v3);
        }
        self.v0 ^= m;
    }

    /// `POSTAMBLE`: fold length/tail byte, then finalize.
    #[inline]
    fn finish(self, c: u32, d: u32) -> u64 {
        let SipState {
            mut v0,
            mut v1,
            mut v2,
            mut v3,
            b,
        } = self;
        v3 ^= b;
        for _ in 0..c {
            sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
        }
        v0 ^= b;
        v2 ^= 0xff;
        for _ in 0..d {
            sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
        }
        (v0 ^ v1) ^ (v2 ^ v3)
    }
}

/// `__siphash_unaligned()` / `siphash()`: SipHash-2-4 over a byte string.
///
/// The input is interpreted as little-endian 8-byte blocks with a
/// little-endian tail, and the message length (mod 256) is mixed into the
/// final block, exactly as in C.
pub fn siphash(data: &[u8], key: &SiphashKey) -> u64 {
    let mut st = SipState::new(&key.key, data.len());
    for chunk in data.chunks_exact(8) {
        let m = u64::from_le_bytes(chunk.try_into().unwrap());
        st.block(m, 2);
    }
    st.b |= tail_le(&data[data.len() - data.len() % 8..]);
    st.finish(2, 4)
}

/// `siphash_1u64()`: SipHash-2-4 PRF value of one `u64`.
pub fn siphash_1u64(first: u64, key: &SiphashKey) -> u64 {
    let mut st = SipState::new(&key.key, 8);
    st.block(first, 2);
    st.finish(2, 4)
}

/// `siphash_2u64()`: SipHash-2-4 PRF value of two `u64`s.
pub fn siphash_2u64(first: u64, second: u64, key: &SiphashKey) -> u64 {
    let mut st = SipState::new(&key.key, 16);
    st.block(first, 2);
    st.block(second, 2);
    st.finish(2, 4)
}

/// `siphash_3u64()`: SipHash-2-4 PRF value of three `u64`s.
pub fn siphash_3u64(first: u64, second: u64, third: u64, key: &SiphashKey) -> u64 {
    let mut st = SipState::new(&key.key, 24);
    st.block(first, 2);
    st.block(second, 2);
    st.block(third, 2);
    st.finish(2, 4)
}

/// `siphash_4u64()`: SipHash-2-4 PRF value of four `u64`s.
pub fn siphash_4u64(first: u64, second: u64, third: u64, forth: u64, key: &SiphashKey) -> u64 {
    let mut st = SipState::new(&key.key, 32);
    st.block(first, 2);
    st.block(second, 2);
    st.block(third, 2);
    st.block(forth, 2);
    st.finish(2, 4)
}

/// `siphash_1u32()`: SipHash-2-4 PRF value of one `u32`
/// (the value lands in the low half of the tail block).
pub fn siphash_1u32(first: u32, key: &SiphashKey) -> u64 {
    let mut st = SipState::new(&key.key, 4);
    st.b |= first as u64;
    st.finish(2, 4)
}

/// Header inline `siphash_2u32()`: packs `(b << 32) | a` and calls
/// `siphash_1u64()`.
pub fn siphash_2u32(first: u32, second: u32, key: &SiphashKey) -> u64 {
    siphash_1u64((second as u64) << 32 | first as u64, key)
}

/// `siphash_3u32()`: first two words packed as one block, third in the tail.
pub fn siphash_3u32(first: u32, second: u32, third: u32, key: &SiphashKey) -> u64 {
    let combined = (second as u64) << 32 | first as u64;
    let mut st = SipState::new(&key.key, 12);
    st.block(combined, 2);
    st.b |= third as u64;
    st.finish(2, 4)
}

/// Header inline `siphash_4u32()`: packs into two `u64`s and calls
/// `siphash_2u64()`.
pub fn siphash_4u32(first: u32, second: u32, third: u32, forth: u32, key: &SiphashKey) -> u64 {
    siphash_2u64(
        (second as u64) << 32 | first as u64,
        (forth as u64) << 32 | third as u64,
        key,
    )
}

/// `__hsiphash_unaligned()` / `hsiphash()` on 64-bit builds: HalfSipHash-1-3
/// over a byte string (i.e. SipHash-1-3, returning the truncated `u32`).
pub fn hsiphash(data: &[u8], key: &HsiphashKey) -> u32 {
    let mut st = SipState::new(&key.key, data.len());
    for chunk in data.chunks_exact(8) {
        let m = u64::from_le_bytes(chunk.try_into().unwrap());
        st.block(m, 1);
    }
    st.b |= tail_le(&data[data.len() - data.len() % 8..]);
    // HPOSTAMBLE: 1 compression round before v0 ^= b, 3 after v2 ^= 0xff;
    // the C function's u64 result is implicitly truncated to u32.
    st.finish(1, 3) as u32
}

/// `hsiphash_1u32()` (64-bit build): tail-block-only variant.
pub fn hsiphash_1u32(first: u32, key: &HsiphashKey) -> u32 {
    let mut st = SipState::new(&key.key, 4);
    st.b |= first as u64;
    st.finish(1, 3) as u32
}

/// `hsiphash_2u32()` (64-bit build): both words packed as one block.
pub fn hsiphash_2u32(first: u32, second: u32, key: &HsiphashKey) -> u32 {
    let combined = (second as u64) << 32 | first as u64;
    let mut st = SipState::new(&key.key, 8);
    st.block(combined, 1);
    st.finish(1, 3) as u32
}

/// `hsiphash_3u32()` (64-bit build): two words as one block, third in tail.
pub fn hsiphash_3u32(first: u32, second: u32, third: u32, key: &HsiphashKey) -> u32 {
    let combined = (second as u64) << 32 | first as u64;
    let mut st = SipState::new(&key.key, 12);
    st.block(combined, 1);
    st.b |= third as u64;
    st.finish(1, 3) as u32
}

/// `hsiphash_4u32()` (64-bit build): two packed blocks.
pub fn hsiphash_4u32(first: u32, second: u32, third: u32, forth: u32, key: &HsiphashKey) -> u32 {
    let combined = (second as u64) << 32 | first as u64;
    let mut st = SipState::new(&key.key, 16);
    st.block(combined, 1);
    let combined = (forth as u64) << 32 | third as u64;
    st.block(combined, 1);
    st.finish(1, 3) as u32
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
