// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's CRC-32 library (`lib/crc/crc32-main.c`,
//! `lib/crc/gen_crc32table.c`, `include/linux/crc32.h`, `include/linux/crc32poly.h`).
//!
//! # C-to-Rust correspondence
//!
//! | C symbol (file)                              | Rust item                          |
//! |----------------------------------------------|------------------------------------|
//! | `CRC32_POLY_LE` (crc32poly.h)                | [`CRC32_POLY_LE`]                  |
//! | `CRC32_POLY_BE` (crc32poly.h)                | [`CRC32_POLY_BE`]                  |
//! | `CRC32C_POLY_LE` (crc32poly.h)               | [`CRC32C_POLY_LE`]                 |
//! | `crc32init_le_generic` (gen_crc32table.c)    | [`crc_table_init_le`]              |
//! | `crc32init_be` (gen_crc32table.c)            | [`be_table_init`]                  |
//! | `crc32table_le` / `crc32ctable_le`           | [`CRC32TABLE_LE`] / [`CRC32CTABLE_LE`] |
//! | `crc32table_be`                              | [`CRC32TABLE_BE`]                  |
//! | `crc32_le_base` (crc32-main.c)               | [`crc32_le_base`]                  |
//! | `crc32_be_base` (crc32-main.c)               | [`crc32_be_base`]                  |
//! | `crc32c_base` (crc32-main.c)                 | [`crc32c_base`]                    |
//! | `crc32_le` (crc32-main.c)                    | [`crc32_le`]                       |
//! | `crc32_be` (crc32-main.c)                    | [`crc32_be`]                       |
//! | `crc32c` (crc32-main.c)                      | [`crc32c`]                         |
//! | `crc32()` alias (crc32.h static inline)      | [`crc32`]                          |
//!
//! # Deviations from C
//!
//! - **Table generation moved to compile time.** The C build generates
//!   `crc32table.h` at build time via the `gen_crc32table.c` host program. Here
//!   the same initialization loops are `const fn`s evaluated during
//!   monomorphization/const-eval, producing bit-identical tables with zero
//!   runtime cost and no generated-code step. Table spot values are pinned
//!   against values produced by a Python transcription of the C generator.
//! - **Slicing-by-8.** The current tree's generic path is byte-at-a-time; the
//!   historical kernel algorithm (see Documentation/staging/crc32.rst) added
//!   slicing-by-4/slicing-by-8 for the reflected variants. This crate uses
//!   slicing-by-8 for [`crc32_le`] and [`crc32c`] (identical results, fewer
//!   iterations per byte) and keeps [`crc32_be`] byte-at-a-time, matching the
//!   current generic path. Differential tests prove slice-by-8 output equals
//!   the base byte-at-a-time implementation on every input class tested.
//! - **No `__crc32c_le_combine`.** It is not present in this tree's headers;
//!   incremental composition is covered by tests instead (the CRC is affine in
//!   its state: crc(A || B) == crc32_le(crc32_le(init, A), B)).
//! - Buffers are Rust slices rather than raw pointers + length; there is no
//!   arch-specific dispatch ([`crc32_optimizations_flags`] always reports no
//!   optimizations for this pure-Rust generic port).

#![no_std]
#![deny(unsafe_code)]

/// The polynomial used by `crc32_le()`, in integer form (crc32poly.h).
pub const CRC32_POLY_LE: u32 = 0xedb88320;
/// The polynomial used by `crc32_be()`, in integer form (crc32poly.h).
pub const CRC32_POLY_BE: u32 = 0x04c11db7;
/// The polynomial used by `crc32c()`, in integer form (crc32poly.h).
pub const CRC32C_POLY_LE: u32 = 0x82f63b78;

/// Port of `crc32init_le_generic()` from lib/crc/gen_crc32table.c:
/// allocate and initialize LE table data.
///
/// crc is the crc of the byte i; other entries are filled in based on the
/// fact that crctable[i^j] = crctable[i] ^ crctable[j].
pub const fn crc_table_init_le(polynomial: u32) -> [u32; 256] {
    let mut tab = [0u32; 256];
    let mut crc: u32 = 1;
    let mut i = 128usize;
    while i != 0 {
        crc = (crc >> 1) ^ if crc & 1 != 0 { polynomial } else { 0 };
        let mut j = 0usize;
        while j < 256 {
            tab[i + j] = crc ^ tab[j];
            j += 2 * i;
        }
        i >>= 1;
    }
    tab
}

/// Port of `crc32init_be()` from lib/crc/gen_crc32table.c.
pub const fn be_table_init() -> [u32; 256] {
    let mut tab = [0u32; 256];
    let mut crc: u32 = 0x8000_0000;
    let mut i = 1usize;
    while i < 256 {
        crc = (crc << 1) ^ if crc & 0x8000_0000 != 0 { CRC32_POLY_BE } else { 0 };
        let mut j = 0usize;
        while j < i {
            tab[i + j] = crc ^ tab[j];
            j += 1;
        }
        i <<= 1;
    }
    tab
}

/// Build the full set of eight slicing-by-8 tables from a base LE table:
/// t_k[i] = t_{k-1}[i] >> 8 ^ t0[t_{k-1}[i] & 0xff], with t_0 the base table.
const fn slice8_tables_init(t0: &[u32; 256]) -> [[u32; 256]; 8] {
    let mut tables = [[0u32; 256]; 8];
    tables[0] = *t0;
    let mut k = 1usize;
    while k < 8 {
        let mut i = 0usize;
        while i < 256 {
            tables[k][i] = (tables[k - 1][i] >> 8) ^ t0[(tables[k - 1][i] & 0xff) as usize];
            i += 1;
        }
        k += 1;
    }
    tables
}

/// `crc32table_le` — the IEEE CRC-32 (reflected) lookup table.
pub const CRC32TABLE_LE: [u32; 256] = crc_table_init_le(CRC32_POLY_LE);
/// `crc32table_be` — the big-endian IEEE CRC-32 lookup table.
pub const CRC32TABLE_BE: [u32; 256] = be_table_init();
/// `crc32ctable_le` — the CRC-32C (Castagnoli, reflected) lookup table.
pub const CRC32CTABLE_LE: [u32; 256] = crc_table_init_le(CRC32C_POLY_LE);

/// Slicing-by-8 tables for [`crc32_le`].
static SLICE8_LE: [[u32; 256]; 8] = slice8_tables_init(&CRC32TABLE_LE);
/// Slicing-by-8 tables for [`crc32c`].
static SLICE8_CRC32C: [[u32; 256]; 8] = slice8_tables_init(&CRC32CTABLE_LE);

/// `crc32_le_base()`: byte-at-a-time least-significant-bit-first IEEE CRC-32.
#[inline]
pub fn crc32_le_base(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc = (crc >> 8) ^ CRC32TABLE_LE[((crc & 0xff) ^ b as u32) as usize];
    }
    crc
}

/// `crc32_be_base()`: byte-at-a-time most-significant-bit-first IEEE CRC-32.
#[inline]
pub fn crc32_be_base(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc = (crc << 8) ^ CRC32TABLE_BE[(((crc >> 24) & 0xff) ^ b as u32) as usize];
    }
    crc
}

/// `crc32c_base()`: byte-at-a-time CRC-32C (Castagnoli).
#[inline]
pub fn crc32c_base(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc = (crc >> 8) ^ CRC32CTABLE_LE[((crc & 0xff) ^ b as u32) as usize];
    }
    crc
}

/// Shared slicing-by-8 driver over precomputed tables. Remainder bytes (<8)
/// are processed through `tail`, which must implement the SAME polynomial as
/// the tables (a previous draft hardcoded the IEEE tail here, silently
/// corrupting every non-multiple-of-8 CRC-32C).
#[inline]
fn crc32_slice8(crc: u32, data: &[u8], t: &[[u32; 256]; 8], tail: fn(u32, &[u8]) -> u32) -> u32 {
    let mut crc = crc;
    let mut chunks = data.chunks_exact(8);
    for c in &mut chunks {
        // XOR the first four bytes into the CRC, then look up each of the
        // eight byte positions in its dedicated table (historical kernel
        // slicing-by-8 formulation).
        let lo = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        crc ^= lo;
        crc = t[7][(crc & 0xff) as usize]
            ^ t[6][((crc >> 8) & 0xff) as usize]
            ^ t[5][((crc >> 16) & 0xff) as usize]
            ^ t[4][((crc >> 24) & 0xff) as usize]
            ^ t[3][c[4] as usize]
            ^ t[2][c[5] as usize]
            ^ t[1][c[6] as usize]
            ^ t[0][c[7] as usize];
    }
    tail(crc, chunks.remainder())
}

/// `crc32_le()`: compute least-significant-bit-first IEEE CRC-32.
///
/// Initial CRC value: `!0` (recommended) or `0` for a new CRC computation, or
/// the previous return value if computing incrementally. This does **not**
/// invert the CRC at the beginning or end; callers are expected to do that if
/// they need it (inverting at both ends is recommended). With both inversions,
/// this is the widely used "IEEE CRC-32" (polynomial 0xedb88320, reflected).
///
/// Context: Any context. Return: The new CRC value.
#[inline]
pub fn crc32_le(crc: u32, data: &[u8]) -> u32 {
    crc32_slice8(crc, data, &SLICE8_LE, crc32_le_base)
}

/// `crc32()`: alias for [`crc32_le()`] (crc32.h static inline).
#[inline]
pub fn crc32(crc: u32, data: &[u8]) -> u32 {
    crc32_le(crc, data)
}

/// `crc32_be()`: compute most-significant-bit-first IEEE CRC-32.
///
/// Same polynomial as [`crc32_le()`], but bits within each byte are processed
/// most-significant-first and the CRC value itself is kept in natural bit
/// order. Implemented byte-at-a-time, matching the current tree's generic
/// path. No initial/final inversion is performed.
#[inline]
pub fn crc32_be(crc: u32, data: &[u8]) -> u32 {
    crc32_be_base(crc, data)
}

/// `crc32c()`: compute CRC-32C (Castagnoli, polynomial 0x82f63b78 reflected).
///
/// The recommended CRC variant for new applications that want a 32-bit CRC.
/// No initial/final inversion is performed by this function itself.
#[inline]
pub fn crc32c(crc: u32, data: &[u8]) -> u32 {
    crc32_slice8(crc, data, &SLICE8_CRC32C, crc32c_base)
}

/// `crc32_optimizations()`: flags indicating which functions use architecture
/// optimizations. This pure-Rust generic port has none.
pub const CRC32_LE_OPTIMIZATION: u32 = 1 << 0;
/// Flag for `crc32_be()` optimization (unused here).
pub const CRC32_BE_OPTIMIZATION: u32 = 1 << 1;
/// Flag for `crc32c()` optimization (unused here).
pub const CRC32C_OPTIMIZATION: u32 = 1 << 2;

/// Always returns 0 in this port (no arch-specific code paths).
#[inline]
pub fn crc32_optimizations_flags() -> u32 {
    0
}

#[cfg(test)]
mod tests;
