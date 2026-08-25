//! Tests ported from the kernel's `lib/test-kstrtox.c`.

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;

use alloc::string::ToString;

use krand::{Krand, Rng};

use super::*;

/// `TEST_OK` equivalent: all entries must parse to the expected value.
fn ok_u(s: &str, base: u32, expected: u64) {
    assert_eq!(kstrtoull(s, base), Ok(expected), "str {s:?} base {base}");
}

#[test]
fn kstrtoull_ok() {
    for v in [0u64, 1, 127, 128, 129, 255, 256, 257] {
        ok_u(&v.to_string(), 10, v);
    }
    for v in [32767u64, 32768, 32769, 65535, 65536, 65537] {
        ok_u(&v.to_string(), 10, v);
    }
    for v in [
        2147483647u64,
        2147483648,
        2147483649,
        4294967295,
        4294967296,
        4294967297,
    ] {
        ok_u(&v.to_string(), 10, v);
    }
    ok_u("9223372036854775807", 10, 9223372036854775807);
    ok_u("9223372036854775808", 10, 9223372036854775808);
    ok_u("9223372036854775809", 10, 9223372036854775809);
    ok_u("18446744073709551614", 10, 18446744073709551614);
    ok_u("18446744073709551615", 10, u64::MAX);

    // Octal table (kernel wrote these as C octal literals).
    let oct: &[(&str, u64)] = &[
        ("00", 0o0),
        ("01", 0o1),
        ("0177", 0o177),
        ("0200", 0o200),
        ("0201", 0o201),
        ("0377", 0o377),
        ("0400", 0o400),
        ("0401", 0o401),
        ("077777", 0o77777),
        ("0100000", 0o100000),
        ("0100001", 0o100001),
        ("0177777", 0o177777),
        ("0200000", 0o200000),
        ("0200001", 0o200001),
        ("017777777777", 0o17777777777),
        ("020000000000", 0o20000000000),
        ("020000000001", 0o20000000001),
        ("037777777777", 0o37777777777),
        ("040000000000", 0o40000000000),
        ("040000000001", 0o40000000001),
        ("0777777777777777777777", (1 << 63) - 1),
        ("01000000000000000000000", 1 << 63),
        ("01000000000000000000001", (1 << 63) + 1),
        ("01777777777777777777776", u64::MAX - 1),
        ("01777777777777777777777", u64::MAX),
    ];
    for (s, v) in oct {
        ok_u(s, 8, *v);
    }

    // Hex table.
    let hex: &[(&str, u64)] = &[
        ("0x0", 0x0),
        ("0x1", 0x1),
        ("0x7f", 0x7f),
        ("0x80", 0x80),
        ("0x81", 0x81),
        ("0xff", 0xff),
        ("0x100", 0x100),
        ("0x101", 0x101),
        ("0x7fff", 0x7fff),
        ("0x8000", 0x8000),
        ("0x8001", 0x8001),
        ("0xffff", 0xffff),
        ("0x10000", 0x10000),
        ("0x10001", 0x10001),
        ("0x7fffffff", 0x7fffffff),
        ("0x80000000", 0x80000000),
        ("0x80000001", 0x80000001),
        ("0xffffffff", 0xffffffff),
        ("0x100000000", 0x100000000),
        ("0x100000001", 0x100000001),
        ("0x7fffffffffffffff", 0x7fffffffffffffff),
        ("0x8000000000000000", 0x8000000000000000),
        ("0x8000000000000001", 0x8000000000000001),
        ("0xfffffffffffffffe", u64::MAX - 1),
        ("0xffffffffffffffff", u64::MAX),
    ];
    for (s, v) in hex {
        ok_u(s, 16, *v);
    }

    ok_u("0\n", 0, 0); // single trailing newline tolerated
}

#[test]
fn kstrtoull_fail() {
    let fail: &[(&str, u32)] = &[
        ("", 0),
        ("", 8),
        ("", 10),
        ("", 16),
        ("\n", 0),
        ("\n", 8),
        ("\n", 10),
        ("\n", 16),
        ("\n0", 0),
        ("\n0", 8),
        ("\n0", 10),
        ("\n0", 16),
        ("+", 0),
        ("+", 8),
        ("+", 10),
        ("+", 16),
        ("-", 0),
        ("-", 8),
        ("-", 10),
        ("-", 16),
        ("0x", 0),
        ("0x", 16),
        ("0X", 0),
        ("0X", 16),
        ("0 ", 0),
        ("1+", 0),
        ("1-", 0),
        (" 2", 0),
        // base autodetection
        ("0x0z", 0),
        ("0z", 0),
        ("a", 0),
        // digit >= base
        ("2", 2),
        ("8", 8),
        ("a", 10),
        ("A", 10),
        ("g", 16),
        ("G", 16),
        // overflow
        (
            "10000000000000000000000000000000000000000000000000000000000000000",
            2,
        ),
        ("2000000000000000000000", 8),
        ("18446744073709551616", 10),
        ("569202370375329612767", 10),
        ("10000000000000000", 16),
        // negative
        ("-0", 0),
        ("-0", 8),
        ("-0", 10),
        ("-0", 16),
        ("-1", 0),
        ("-1", 8),
        ("-1", 10),
        ("-1", 16),
        // sign is first character if any
        ("-+1", 0),
        ("-+1", 8),
        ("-+1", 10),
        ("-+1", 16),
        // nothing after \n
        ("0\n0", 0),
        ("0\n0", 8),
        ("0\n0", 10),
        ("0\n0", 16),
        ("0\n+", 0),
        ("0\n+", 8),
        ("0\n+", 10),
        ("0\n+", 16),
        ("0\n-", 0),
        ("0\n-", 8),
        ("0\n-", 10),
        ("0\n-", 16),
        ("0\n ", 0),
        ("0\n ", 8),
        ("0\n ", 10),
        ("0\n ", 16),
    ];
    for (s, base) in fail {
        assert!(
            kstrtoull(s, *base).is_err(),
            "str {s:?} base {base} expected error"
        );
    }
}

#[test]
fn kstrtoll_ok() {
    assert_eq!(kstrtoll("0", 10), Ok(0));
    assert_eq!(kstrtoll("127", 10), Ok(127));
    assert_eq!(kstrtoll("-1", 10), Ok(-1));
    assert_eq!(kstrtoll("-2", 10), Ok(-2));
    assert_eq!(kstrtoll("-0", 10), Ok(0));
    assert_eq!(kstrtoll("2147483647", 10), Ok(2147483647));
    assert_eq!(kstrtoll("4294967296", 10), Ok(4294967296));
    assert_eq!(kstrtoll("9223372036854775807", 10), Ok(i64::MAX));
    assert_eq!(kstrtoll("-9223372036854775808", 10), Ok(i64::MIN));
    // leading + accepted
    assert_eq!(kstrtoll("+42", 10), Ok(42));
}

#[test]
fn kstrtoll_fail() {
    for s in [
        "9223372036854775808",
        "9223372036854775809",
        "18446744073709551614",
        "18446744073709551615",
        "569202370375329612767",
        "-9223372036854775809",
        "-18446744073709551614",
        "-18446744073709551615",
        "-569202370375329612767",
        // sign is first character if any
        "-+1",
    ] {
        assert!(kstrtoll(s, 10).is_err(), "str {s:?} expected error");
    }
    for base in [0u32, 8, 10, 16] {
        assert!(kstrtoll("-+1", base).is_err());
    }
}

#[test]
fn narrow_types() {
    // u8 boundaries
    assert_eq!(kstrtou8("255", 10), Ok(255));
    assert_eq!(kstrtou8("256", 10), Err(Error::Range));
    assert_eq!(kstrtou8("-1", 10), Err(Error::Invalid)); // C: kstrtoull rejects '-' first (-EINVAL)
                                                         // s8 boundaries
    assert_eq!(kstrtos8("-128", 10), Ok(-128));
    assert_eq!(kstrtos8("127", 10), Ok(127));
    assert_eq!(kstrtos8("128", 10), Err(Error::Range));
    assert_eq!(kstrtos8("-129", 10), Err(Error::Range));
    // u16 boundaries
    assert_eq!(kstrtou16("65535", 10), Ok(65535));
    assert_eq!(kstrtou16("65536", 10), Err(Error::Range));
    // s16 boundaries
    assert_eq!(kstrtos16("-130", 10), Ok(-130));
    assert_eq!(kstrtos16("32767", 10), Ok(32767));
    assert_eq!(kstrtos16("32768", 10), Err(Error::Range));
    // u32/i32 boundaries
    assert_eq!(kstrtouint("4294967295", 10), Ok(u32::MAX));
    assert_eq!(kstrtouint("4294967296", 10), Err(Error::Range));
    assert_eq!(kstrtoint("2147483647", 10), Ok(i32::MAX));
    assert_eq!(kstrtoint("2147483648", 10), Err(Error::Range));
    assert_eq!(kstrtoint("-2147483648", 10), Ok(i32::MIN));
}

#[test]
fn kstrtobool_ok() {
    for s in ["E", "e", "y", "Y", "t", "T", "1"] {
        assert_eq!(kstrtobool(s), Ok(true), "{s:?}");
    }
    for s in ["D", "d", "n", "N", "f", "F", "0"] {
        assert_eq!(kstrtobool(s), Ok(false), "{s:?}");
    }
    for s in ["on", "oN", "On", "ON"] {
        assert_eq!(kstrtobool(s), Ok(true), "{s:?}");
    }
    for s in ["of", "oF", "Of", "OF"] {
        assert_eq!(kstrtobool(s), Ok(false), "{s:?}");
    }
}

#[test]
fn kstrtobool_fail() {
    for s in ["", "x", "oo", "2", "-1"] {
        assert_eq!(kstrtobool(s), Err(Error::Invalid), "{s:?}");
    }
}

/// Randomized cross-check against `from_str_radix` for every base 2..=16.
/// Values are generated by drawing random digits in the chosen base, so
/// most iterations land near the overflow boundary of the target type.
#[test]
fn differential_vs_from_str_radix_randomized() {
    use alloc::string::String;

    let mut rng = Krand::seed_from_u64(0x5EED_1234);
    for _ in 0..5_000 {
        let base = rng.below(15) + 2; // 2..=16, matching the C API max
        let digits = "0123456789abcdef";
        let len = rng.below(20) as usize;
        let s: String = (0..len)
            .map(|_| {
                let b = digits.as_bytes()[rng.below(base) as usize];
                b as char
            })
            .collect();
        // The kernel API accepts an optional leading '+'.
        let signed: String = if rng.coin_flip() {
            alloc::format!("+{s}")
        } else {
            s.clone()
        };

        match u64::from_str_radix(&s, base as u32) {
            Ok(want) => {
                assert_eq!(
                    kstrtoull(&signed, base as u32),
                    Ok(want),
                    "s={s:?} base={base}"
                );
                if want <= i64::MAX as u64 {
                    assert_eq!(
                        kstrtoll(&signed, base as u32),
                        Ok(want as i64),
                        "s={s:?} base={base}"
                    );
                }
            }
            Err(_) => {
                // Digits are always valid for the base here, so std only
                // rejects on overflow or on the empty string. The kernel API
                // maps those to Range and Invalid respectively.
                let want = if s.is_empty() {
                    Error::Invalid
                } else {
                    Error::Range
                };
                assert_eq!(
                    kstrtoull(&signed, base as u32),
                    Err(want),
                    "s={s:?} base={base}"
                );
            }
        }
    }

    // Also verify the trailing-newline tolerance on random valid inputs.
    let mut rng = Krand::seed_from_u64(0x5EED_4321);
    for _ in 0..500 {
        let v = rng.next_u64();
        let s: String = alloc::format!("{v}\n");
        assert_eq!(kstrtoull(&s, 10), Ok(v));
    }
}

/// Cross-check against Rust's own parser for decimal values (differential test).
#[test]
fn differential_vs_std_decimal() {
    // Sweep every power-of-two boundary ±1 across the full u64 range.
    for shift in 0..64u32 {
        for delta in [-1i64, 0, 1] {
            let v = (1u128 << shift) as i128 + delta as i128;
            if v < 0 || v > u64::MAX as i128 {
                continue;
            }
            let s = v.to_string();
            assert_eq!(
                kstrtoull(&s, 10),
                Ok(v as u64),
                "shift {shift} delta {delta}"
            );
        }
    }
}
