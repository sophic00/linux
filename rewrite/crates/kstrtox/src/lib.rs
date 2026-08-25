// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's `lib/kstrtox.c`.
//!
//! Convert integer string representation to an integer. If an integer does
//! not fit into the specified type, [`Error::Range`] is returned.
//!
//! Integer starts with an optional sign. The `kstrtou*` functions do not
//! accept a leading `-`.
//!
//! Radix 0 means autodetection: a leading `0x` implies radix 16, a leading
//! `0` implies radix 8, otherwise radix is 10. Autodetection hints work
//! after an optional sign, but not before.
//!
//! On error, no value is produced (unlike the C out-parameter API, Rust's
//! `Result` makes this impossible to misuse).

#![no_std]
#![deny(unsafe_code)]

/// Kernel errno counterparts, kept for parity with the C implementation.
pub const EINVAL: i32 = 22;
/// Kernel errno for "result out of range".
pub const ERANGE: i32 = 34;

/// Error type mirroring the negative-errno returns of the C `kstrto*()` API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Parsing error (C: `-EINVAL`).
    Invalid,
    /// Value out of range for the target type (C: `-ERANGE`).
    Range,
}

impl Error {
    /// Negative errno matching the C return value.
    pub const fn errno(self) -> i32 {
        match self {
            Error::Invalid => -EINVAL,
            Error::Range => -ERANGE,
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Invalid => f.write_str("invalid argument"),
            Error::Range => f.write_str("value out of range"),
        }
    }
}

/// `_parse_integer_fixup_radix()`: resolve base 0 via prefix autodetection and
/// strip a redundant `0x` when base is explicitly 16.
fn parse_integer_fixup_radix<'a>(mut s: &'a [u8], base: &mut u32) -> &'a [u8] {
    if *base == 0 {
        if s.first() == Some(&b'0') {
            let x = s.get(1).map_or(b'\0', |c| c.to_ascii_lowercase());
            let third_is_xdigit = s.get(2).is_some_and(u8::is_ascii_hexdigit);
            if x == b'x' && third_is_xdigit {
                *base = 16;
            } else {
                *base = 8;
            }
        } else {
            *base = 10;
        }
    }
    if *base == 16 && s.first() == Some(&b'0') && s.get(1).map_or(b'\0', |c| c.to_ascii_lowercase()) == b'x'
    {
        s = &s[2..];
    }
    s
}

/// Return-value flag of the digit scanner: overflow occurred. Mirrors
/// `KSTRTOX_OVERFLOW` in `lib/kstrtox.h`, ORed into the consumed-count.
const KSTRTOX_OVERFLOW: u32 = 1 << 31;

/// `_parse_integer_limit()`: convert a non-negative integer string in the
/// given radix, consuming at most `max_chars` digits.
///
/// Returns `(rv, res)` where `rv` is the number of characters consumed,
/// possibly ORed with [`KSTRTOX_OVERFLOW`] (in which case `res` saturates to
/// `u64::MAX`, exactly like the C version).
fn parse_integer_limit(s: &[u8], base: u32, max_chars: usize) -> (u32, u64) {
    let mut rv: u32 = 0;
    let mut res: u64 = 0;
    let mut overflow = false;

    for &c in s.iter().take(max_chars) {
        let lc = c.to_ascii_lowercase();
        let val = match c {
            b'0'..=b'9' => (c - b'0') as u32,
            _ if (b'a'..=b'f').contains(&lc) => (lc - b'a') as u32 + 10,
            _ => break,
        };
        if val >= base {
            break;
        }
        // Check for overflow only within range of it for the max base (16).
        if res & (u64::MAX << 60) != 0 {
            match res.checked_mul(base as u64).and_then(|r| r.checked_add(val as u64)) {
                Some(r) => res = r,
                None => {
                    res = u64::MAX;
                    overflow = true;
                }
            }
        } else {
            res = res * base as u64 + val as u64;
        }
        rv += 1;
    }

    (if overflow { rv | KSTRTOX_OVERFLOW } else { rv }, res)
}

/// `_kstrtoull()`: shared worker for `kstrtoull`/`kstrtoll`.
fn kstrtoull_inner(s: &[u8], mut base: u32) -> Result<u64, Error> {
    let s = parse_integer_fixup_radix(s, &mut base);
    let (rv, res) = parse_integer_limit(s, base, i32::MAX as usize);
    if rv & KSTRTOX_OVERFLOW != 0 {
        return Err(Error::Range);
    }
    if rv == 0 {
        return Err(Error::Invalid);
    }
    let mut rest = &s[rv as usize..];
    if rest.first() == Some(&b'\n') {
        rest = &rest[1..];
    }
    if !rest.is_empty() {
        return Err(Error::Invalid);
    }
    Ok(res)
}

/// `kstrtoull()`: convert a string to an unsigned long long.
///
/// The string may include a single trailing newline; a single leading `+` is
/// accepted, but not `-`.
pub fn kstrtoull(s: &str, base: u32) -> Result<u64, Error> {
    let bytes = s.as_bytes();
    let bytes = if bytes.first() == Some(&b'+') { &bytes[1..] } else { bytes };
    kstrtoull_inner(bytes, base)
}

/// `kstrtoll()`: convert a string to a long long.
///
/// A single leading `+` or `-` sign is accepted.
pub fn kstrtoll(s: &str, base: u32) -> Result<i64, Error> {
    let bytes = s.as_bytes();
    if bytes.first() == Some(&b'-') {
        let tmp = kstrtoull_inner(&bytes[1..], base)?;
        // C checks `(long long)-tmp > 0`; the safe equivalent of that test:
        // |tmp| may reach exactly 2^63 (i64::MIN), but no further.
        if tmp > (i64::MAX as u64) + 1 {
            return Err(Error::Range);
        }
        Ok((tmp as i128).wrapping_neg() as i64)
    } else {
        let tmp = kstrtoull(s, base)?;
        if tmp > i64::MAX as u64 {
            return Err(Error::Range);
        }
        Ok(tmp as i64)
    }
}

macro_rules! unsigned_conv {
    ($(#[$doc:meta])* $name:ident, $t:ty) => {
        $(#[$doc])*
        pub fn $name(s: &str, base: u32) -> Result<$t, Error> {
            let tmp = kstrtoull(s, base)?;
            <$t>::try_from(tmp).map_err(|_| Error::Range)
        }
    };
}

macro_rules! signed_conv {
    ($(#[$doc:meta])* $name:ident, $t:ty) => {
        $(#[$doc])*
        pub fn $name(s: &str, base: u32) -> Result<$t, Error> {
            let tmp = kstrtoll(s, base)?;
            <$t>::try_from(tmp).map_err(|_| Error::Range)
        }
    };
}

unsigned_conv! {
    /// `kstrtouint()`: convert a string to an unsigned int.
    kstrtouint, u32
}
unsigned_conv! {
    /// `kstrtou16()`: convert a string to a u16.
    kstrtou16, u16
}
unsigned_conv! {
    /// `kstrtou8()`: convert a string to a u8.
    kstrtou8, u8
}
signed_conv! {
    /// `kstrtoint()`: convert a string to an int.
    kstrtoint, i32
}
signed_conv! {
    /// `kstrtos16()`: convert a string to a s16.
    kstrtos16, i16
}
signed_conv! {
    /// `kstrtos8()`: convert a string to a s8.
    kstrtos8, i8
}

/// `kstrtobool()`: convert common user inputs into boolean values.
///
/// Accepts first characters from `[EeYyTt1DdNnFf0]`, or `[oO][NnFf]` for
/// "on" and "off". Anything else is [`Error::Invalid`].
pub fn kstrtobool(s: &str) -> Result<bool, Error> {
    let b = s.as_bytes();
    match b.first() {
        None => Err(Error::Invalid),
        Some(c @ (b'e' | b'E' | b'y' | b'Y' | b't' | b'T' | b'1')) => {
            let _ = c;
            Ok(true)
        }
        Some(c @ (b'd' | b'D' | b'n' | b'N' | b'f' | b'F' | b'0')) => {
            let _ = c;
            Ok(false)
        }
        Some(b'o' | b'O') => match b.get(1) {
            Some(b'n' | b'N') => Ok(true),
            Some(b'f' | b'F') => Ok(false),
            _ => Err(Error::Invalid),
        },
        _ => Err(Error::Invalid),
    }
}

#[cfg(test)]
mod tests;
