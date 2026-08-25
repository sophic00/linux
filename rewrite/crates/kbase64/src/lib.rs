// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's `lib/base64.c` (base64 encoding with
//! support for multiple variants).
//!
//! Based on the base64url routines from fs/crypto/fname.c, modified upstream
//! to support multiple Base64 variants (RFC 4648 standard, RFC 4648 base64url,
//! RFC 3501 IMAP mailbox name encoding).
//!
//! Deviations from the C API, all documented:
//! - The C functions assume the caller sized `dst` correctly and would
//!   otherwise overflow. Here an undersized output buffer returns
//!   [`Error::Invalid`] instead (the C code has no such error for encode;
//!   decode's C `-1` return maps to [`Error::Invalid`] as well).
//! - The C decode takes a `(char *, int)` pair; here that is one `&[u8]`,
//!   which may contain interior NUL bytes exactly like the C version
//!   (see the `with_nul` test ported from lib/tests/base64_kunit.c).
//! - The KUnit benchmark cases in lib/tests/base64_kunit.c are intentionally
//!   not ported; they measure timing, not semantics.

#![no_std]
#![deny(unsafe_code)]

/// Kernel `-EINVAL`, kept for parity with the C implementation's error
/// convention (`-1` in lib/base64.c maps to the same "invalid" condition).
pub const EINVAL: i32 = 22;

/// Error type mirroring negative-errno returns. `lib/base64.c` only ever
/// fails with "invalid input", so this enum has a single variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Invalid argument: malformed input or undersized output buffer
    /// (C: `-1` from `base64_decode()`; encode never fails in C).
    Invalid,
}

impl Error {
    /// Negative errno matching the conventional C return value.
    pub const fn errno(self) -> i32 {
        -EINVAL
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("invalid base64 input or undersized output buffer")
    }
}

/// Which Base64 alphabet to use. Mirrors `enum base64_variant`
/// in include/linux/base64.h.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base64Variant {
    /// RFC 4648 (standard).
    Std,
    /// RFC 4648 (base64url): `-` and `_` instead of `+` and `/`.
    Urlsafe,
    /// RFC 3501 (IMAP mailbox names): `,` instead of `/`.
    Imap,
}

const BASE64_TABLES: [&[u8; 64]; 3] = [
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,",
];

const INVALID: i8 = -1;

/// One entry of the reverse mapping table, mirroring the C INIT_1 macro:
/// 'A'-'Z' -> 0-25, 'a'-'z' -> 26-51, '0'-'9' -> 52-61, ch62 -> 62,
/// ch63 -> 63, everything else -> -1.
const fn rev_entry(v: u8, ch_62: u8, ch_63: u8) -> i8 {
    match v {
        b'A'..=b'Z' => (v - b'A') as i8,
        b'a'..=b'z' => (v - b'a') as i8 + 26,
        b'0'..=b'9' => (v - b'0') as i8 + 52,
        _ if v == ch_62 => 62,
        _ if v == ch_63 => 63,
        _ => INVALID,
    }
}

/// Builds the 256-entry reverse table. The C initializer only populates
/// 0x20..=0x7f (via INIT_32 at 0x20/0x40/0x60); everything else stays -1.
const fn init_rev(ch_62: u8, ch_63: u8) -> [i8; 256] {
    let mut t = [INVALID; 256];
    let mut v = 0x20usize;
    while v <= 0x7f {
        t[v] = rev_entry(v as u8, ch_62, ch_63);
        v += 1;
    }
    t
}

const BASE64_REV_MAPS: [[i8; 256]; 3] = [
    init_rev(b'+', b'/'),
    init_rev(b'-', b'_'),
    init_rev(b'+', b','),
];

/// `BASE64_CHARS(nbytes)` from include/linux/base64.h: conservative upper
/// bound on the encoded length (it ignores that padding replaces rather than
/// adds characters). Useful for sizing destination buffers.
pub const fn base64_chars(nbytes: usize) -> usize {
    (nbytes * 4).div_ceil(3)
}

/// `base64_encode()`: Base64-encode binary data into `dst`.
///
/// Encodes `src` using the selected variant. The output is *not*
/// NUL-terminated. Returns the number of bytes written to `dst`.
///
/// Unlike the C function (which trusts the caller's buffer size), returns
/// [`Error::Invalid`] if `dst` is smaller than the exact encoded length.
pub fn encode(
    src: &[u8],
    dst: &mut [u8],
    padding: bool,
    variant: Base64Variant,
) -> Result<usize, Error> {
    let full = src.len() / 3;
    let rem = src.len() % 3;
    let outlen =
        full * 4 + match rem {
            0 => 0,
            1 => {
                if padding {
                    4
                } else {
                    2
                }
            }
            _ => {
                if padding {
                    4
                } else {
                    3
                }
            }
        };
    if dst.len() < outlen {
        return Err(Error::Invalid);
    }

    let table = &BASE64_TABLES[variant as usize];
    let mut cp = 0usize;

    for ch in src.chunks_exact(3) {
        // u32 casts before shifting avoid overflowing u8 arithmetic; the C
        // code relies on int promotion for the same effect.
        let ac = ((ch[0] as u32) << 16) | ((ch[1] as u32) << 8) | ch[2] as u32;
        dst[cp] = table[(ac >> 18) as usize];
        dst[cp + 1] = table[((ac >> 12) & 0x3f) as usize];
        dst[cp + 2] = table[((ac >> 6) & 0x3f) as usize];
        dst[cp + 3] = table[(ac & 0x3f) as usize];
        cp += 4;
    }

    let rest = &src[full * 3..];
    match rem {
        2 => {
            let ac = ((rest[0] as u32) << 16) | ((rest[1] as u32) << 8);
            dst[cp] = table[(ac >> 18) as usize];
            dst[cp + 1] = table[((ac >> 12) & 0x3f) as usize];
            dst[cp + 2] = table[((ac >> 6) & 0x3f) as usize];
            cp += 3;
            if padding {
                dst[cp] = b'=';
                cp += 1;
            }
        }
        1 => {
            let ac = (rest[0] as u32) << 16;
            dst[cp] = table[(ac >> 18) as usize];
            dst[cp + 1] = table[((ac >> 12) & 0x3f) as usize];
            cp += 2;
            if padding {
                dst[cp] = b'=';
                dst[cp + 1] = b'=';
                cp += 2;
            }
        }
        _ => {}
    }

    Ok(cp)
}

/// `base64_decode()`: Base64-decode a string into `dst`.
///
/// Decodes `src` (which need not be NUL-terminated and may contain interior
/// NULs, exactly like the C version) using the selected variant.
///
/// When `padding` is true the input is expected to use '=' padding; when
/// false, '=' anywhere is invalid.
///
/// Returns the number of bytes written to `dst`, or [`Error::Invalid`] where
/// the C function would return -1 (or overflow the caller's buffer).
pub fn decode(
    src: &[u8],
    dst: &mut [u8],
    padding: bool,
    variant: Base64Variant,
) -> Result<usize, Error> {
    // Conservative capacity bound: every 4 input chars yield <= 3 bytes,
    // plus at most 2 more for a trailing partial group.
    let cap = src.len().div_ceil(4) * 3;
    if dst.len() < cap {
        return Err(Error::Invalid);
    }

    let rev = &BASE64_REV_MAPS[variant as usize];
    let mut bp = 0usize;
    let mut s = src;
    let mut srclen = s.len();
    let mut padding = padding;

    while srclen >= 4 {
        // i8 lookups sign-extend into i32; any invalid character contributes
        // (-1), setting high bits so `val < 0` detects it — same trick as C.
        let val = (rev[s[0] as usize] as i32) << 18
            | (rev[s[1] as usize] as i32) << 12
            | (rev[s[2] as usize] as i32) << 6
            | rev[s[3] as usize] as i32;

        if val < 0 {
            if !padding || srclen != 4 || s[3] != b'=' {
                return Err(Error::Invalid);
            }
            padding = false;
            srclen = if s[2] == b'=' { 2 } else { 3 };
            break;
        }

        dst[bp] = (val >> 16) as u8;
        dst[bp + 1] = (val >> 8) as u8;
        dst[bp + 2] = val as u8;
        bp += 3;

        s = &s[4..];
        srclen -= 4;
    }

    if srclen == 0 {
        return Ok(bp);
    }
    if padding || srclen == 1 {
        return Err(Error::Invalid);
    }

    let mut val = ((rev[s[0] as usize] as i32) << 12) | ((rev[s[1] as usize] as i32) << 6);

    if srclen == 2 {
        if (val as u32) & 0x8000_03ff != 0 {
            return Err(Error::Invalid);
        }
        dst[bp] = (val >> 10) as u8;
        bp += 1;
    } else {
        val |= rev[s[2] as usize] as i32;
        if (val as u32) & 0x8000_0003 != 0 {
            return Err(Error::Invalid);
        }
        dst[bp] = (val >> 10) as u8;
        dst[bp + 1] = (val >> 2) as u8;
        bp += 2;
    }

    Ok(bp)
}

#[cfg(test)]
mod tests;
