//! Tests for the `lib/hexdump.c` rewrite.
//!
//! Vector tables and both the "prepare_test" oracle and the overflow
//! expectations are ported from the kernel's own `lib/test_hexdump.c`.
//! The kernel suite randomizes lengths/rowsizes; here the same parameter
//! space is swept *exhaustively* (a deterministic superset of the kernel's
//! randomized runs). An independently structured naive formatter provides a
//! second differential oracle.

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::*;

/// `data_b` from lib/test_hexdump.c.
const DATA_B: [u8; 32] = [
    0xbe, 0x32, 0xdb, 0x7b, 0x0a, 0x18, 0x93, 0xb2, // 00 - 07
    0x70, 0xba, 0xc4, 0x24, 0x7d, 0x83, 0x34, 0x9b, // 08 - 0f
    0xa6, 0x9c, 0x31, 0xad, 0x9c, 0x0f, 0xac, 0xe9, // 10 - 17
    0x4c, 0xd1, 0x19, 0x99, 0x43, 0xb1, 0xaf, 0x0c, // 18 - 1f
];

/// `data_a` from lib/test_hexdump.c (ASCII rendering of DATA_B).
const DATA_A: &[u8; 32] = b".2.{....p..$}.4...1.....L...C...";

const TEST_DATA_1: &[&str] = &[
    "be", "32", "db", "7b", "0a", "18", "93", "b2", //
    "70", "ba", "c4", "24", "7d", "83", "34", "9b", //
    "a6", "9c", "31", "ad", "9c", "0f", "ac", "e9", //
    "4c", "d1", "19", "99", "43", "b1", "af", "0c",
];

const TEST_DATA_2_LE: &[&str] = &[
    "32be", "7bdb", "180a", "b293", //
    "ba70", "24c4", "837d", "9b34", //
    "9ca6", "ad31", "0f9c", "e9ac", //
    "d14c", "9919", "b143", "0caf",
];

const TEST_DATA_2_BE: &[&str] = &[
    "be32", "db7b", "0a18", "93b2", //
    "70ba", "c424", "7d83", "349b", //
    "a69c", "31ad", "9c0f", "ace9", //
    "4cd1", "1999", "43b1", "af0c",
];

const TEST_DATA_4_LE: &[&str] = &[
    "7bdb32be", "b293180a", "24c4ba70", "9b34837d", //
    "ad319ca6", "e9ac0f9c", "9919d14c", "0cafb143",
];

const TEST_DATA_4_BE: &[&str] = &[
    "be32db7b", "0a1893b2", "70bac424", "7d83349b", //
    "a69c31ad", "9c0face9", "4cd11999", "43b1af0c",
];

const TEST_DATA_8_LE: &[&str] = &[
    "b293180a7bdb32be",
    "9b34837d24c4ba70", //
    "e9ac0f9cad319ca6",
    "0cafb1439919d14c",
];

const TEST_DATA_8_BE: &[&str] = &[
    "be32db7b0a1893b2",
    "70bac4247d83349b", //
    "a69c31ad9c0face9",
    "4cd1199943b1af0c",
];

fn is_be() -> bool {
    cfg!(target_endian = "big")
}

const FILL_CHAR: u8 = b'#';
const TEST_HEXDUMP_BUF_SIZE: usize = 32 * 3 + 2 + 32 + 1;

/// Port of `test_hexdump_prepare_test()`: build the expected buffer bytes
/// (including FILL_CHAR tail), returning `(bytes_without_nul, nul_pos)`.
fn prepare_test(
    len: usize,
    rowsize: usize,
    groupsize: usize,
    ascii: bool,
) -> ([u8; TEST_HEXDUMP_BUF_SIZE], usize) {
    let l = len;
    let rs = if rowsize != 16 && rowsize != 32 {
        16
    } else {
        rowsize
    };
    let l = l.min(rs);
    let gs = if !is_power_of_2(groupsize) || groupsize > 8 || len % groupsize != 0 {
        1
    } else {
        groupsize
    };

    let be = is_be();
    let result: &[&str] = match gs {
        8 => {
            if be {
                TEST_DATA_8_BE
            } else {
                TEST_DATA_8_LE
            }
        }
        4 => {
            if be {
                TEST_DATA_4_BE
            } else {
                TEST_DATA_4_LE
            }
        }
        2 => {
            if be {
                TEST_DATA_2_BE
            } else {
                TEST_DATA_2_LE
            }
        }
        _ => TEST_DATA_1,
    };

    let mut test = [FILL_CHAR; TEST_HEXDUMP_BUF_SIZE];
    let mut p = 0usize;

    // hex dump groups, space after each, then remove trailing space
    for q in result.iter().take(l / gs) {
        let q = q.as_bytes();
        test[p..p + q.len()].copy_from_slice(q);
        p += q.len();
        test[p] = b' ';
        p += 1;
    }
    if l / gs != 0 {
        p -= 1;
    }

    // ASCII part
    if ascii {
        let col = rs * 2 + rs / gs + 1;
        while p < col {
            test[p] = b' ';
            p += 1;
        }
        test[p..p + l].copy_from_slice(&DATA_A[..l]);
        p += l;
    }

    test[p] = 0;
    (test, p)
}

/// Port of `test_hexdump()`: full-buffer byte comparison against the oracle.
fn check_hexdump(len: usize, rowsize: usize, groupsize: usize, ascii: bool) {
    // C passes (data_b, len) explicitly; the slice carries the length here.
    let input = &DATA_B[..len.min(DATA_B.len())];
    let mut real = [FILL_CHAR; TEST_HEXDUMP_BUF_SIZE];
    let r = hex_dump_to_buffer(input, rowsize, groupsize, &mut real, ascii);
    let (expect, nul_pos) = prepare_test(len, rowsize, groupsize, ascii);

    assert_eq!(
        r, nul_pos as isize,
        "len={len} row={rowsize} group={groupsize} ascii={ascii}"
    );
    assert_eq!(
        real, expect,
        "len={len} row={rowsize} group={groupsize} ascii={ascii}"
    );
}

#[test]
fn ported_kernel_suite_exhaustive() {
    // Kernel randomizes len in [1, min(32, rs)]; sweep it exhaustively.
    for rs in [16usize, 32] {
        for ascii in [false, true] {
            for gs in [1usize, 2, 4, 8] {
                for len in 1..=rs.min(DATA_B.len()) {
                    check_hexdump(len, rs, gs, ascii);
                }
            }
        }
    }
}

#[test]
fn normalization_paths() {
    // rowsize not 16/32 -> 16; groupsize not power-of-2 or > 8 -> 1;
    // groupsize that doesn't divide len -> 1. Verify against the oracle.
    for rs in [0usize, 1, 15, 17, 31, 33, 64, 1000000] {
        for gs in [0usize, 3, 5, 6, 7, 9, 12, 16, 100] {
            for ascii in [false, true] {
                for len in [1usize, 2, 3, 5, 8, 15, 16, 17, 31, 32] {
                    check_hexdump(len, rs, gs, ascii);
                }
            }
        }
    }
}

/// Port of `test_hexdump_overflow()` logic: exact return value plus exact
/// truncated buffer bytes for every buflen.
fn check_overflow(buflen: usize, len: usize, rs: usize, gs_in: usize, ascii: bool) {
    // Caller provides len multiple of groupsize (kernel harness assumption).
    let ae = rs * 2 /* hex */ + rs / gs_in /* spaces */ + 1 /* space */ + len /* ascii */;
    let he = (gs_in * 2 /* hex */ + 1/* space */) * len / gs_in - 1; /* no trailing space */
    let e: isize = if ascii { ae as isize } else { he as isize };

    let f = (e as usize + 1).min(buflen);

    let mut buf = [FILL_CHAR; TEST_HEXDUMP_BUF_SIZE];
    let input = &DATA_B[..len.min(DATA_B.len())];
    let r = hex_dump_to_buffer(input, rs, gs_in, &mut buf[..buflen], ascii);

    assert_eq!(
        r, e,
        "len={len} buflen={buflen} row={rs} group={gs_in} ascii={ascii}"
    );

    if buflen != 0 {
        let (mut test, _) = prepare_test(len, rs, gs_in, ascii);
        test[f - 1] = 0;
        let mut expect = [FILL_CHAR; TEST_HEXDUMP_BUF_SIZE];
        expect[..TEST_HEXDUMP_BUF_SIZE.min(f)]
            .copy_from_slice(&test[..TEST_HEXDUMP_BUF_SIZE.min(f)]);
        assert_eq!(
            buf, expect,
            "len={len} buflen={buflen} row={rs} group={gs_in} ascii={ascii}"
        );
    }
}

#[test]
fn ported_kernel_overflow_suite_exhaustive() {
    // Kernel sweeps buflen 0..=TEST_HEXDUMP_BUF_SIZE with randomized
    // (rs, gs, len); we sweep the whole cross-product deterministically:
    // len runs over every multiple of gs up to one row past the cap.
    for buflen in 0..=TEST_HEXDUMP_BUF_SIZE {
        for rs in [16usize, 32] {
            for gi in 0..4usize {
                let gs = 1usize << gi;
                // Kernel harness: len is a random multiple of gs below
                // rs+gs, rounded down -- i.e. always <= rs after capping.
                // Sweep every such multiple deterministically.
                let mut len = gs;
                while len <= rs {
                    check_overflow(buflen, len, rs, gs, false);
                    check_overflow(buflen, len, rs, gs, true);
                    len += gs;
                }
            }
        }
    }
}

#[test]
fn linebuflen_zero_quirks() {
    let mut buf = [FILL_CHAR; TEST_HEXDUMP_BUF_SIZE];
    // Documented C quirk: empty input + no room + !ascii returns -1
    // ((gs*2+1)*ngroups - 1 with ngroups == 0 wraps to -1 in int arithmetic).
    assert_eq!(hex_dump_to_buffer(&[], 16, 1, &mut buf[..0], false), -1);
    // ascii variant reports the would-be ascii column instead.
    assert_eq!(
        hex_dump_to_buffer(&[], 16, 1, &mut buf[..0], true),
        16 * 2 + 16 + 1
    );
    // Empty linebuf but non-empty data still reports would-be length.
    assert_eq!(
        hex_dump_to_buffer(&DATA_B[..4], 16, 1, &mut buf[..0], false),
        (3 * 4 - 1)
    );
}

// ---------------------------------------------------------------------------
// Independent differential oracle #2: a naive formatter written directly
// from the documented output format, sharing no code path with either the
// implementation or prepare_test().
// ---------------------------------------------------------------------------

fn naive_format(input: &[u8], rowsize: usize, groupsize_in: usize, ascii: bool) -> String {
    let rs = if rowsize != 16 && rowsize != 32 {
        16
    } else {
        rowsize
    };
    let len = input.len().min(rs);
    // C: "if (!len) goto nil" — an empty line stays empty even in ascii
    // mode (no column padding).
    if len == 0 {
        return String::new();
    }
    let input = &input[..len];
    let gs = if !is_power_of_2(groupsize_in) || groupsize_in > 8 || len % groupsize_in != 0 {
        1
    } else {
        groupsize_in
    };

    let mut out = String::new();

    if gs == 1 {
        for (i, b) in input.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(format!("{:02x}", b).as_str());
        }
    } else {
        for g in 0..len / gs {
            if g > 0 {
                out.push(' ');
            }
            let chunk = &input[g * gs..(g + 1) * gs];
            // Native-endian group value, zero-padded to 2*gs digits.
            let v: u64 = chunk
                .iter()
                .enumerate()
                .map(|(i, &b)| (b as u64) << (8 * i)) // little-endian assembly of NE loads
                .fold(0, |acc, x| acc | x);
            let be_v: u64 = if cfg!(target_endian = "big") {
                chunk.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64)
            } else {
                v
            };
            out.push_str(format!("{:0width$x}", be_v, width = gs * 2).as_str());
        }
    }

    if ascii {
        let column = rs * 2 + rs / gs + 1;
        while out.len() < column {
            out.push(' ');
        }
        for &b in input {
            out.push(if (0x20..=0x7e).contains(&b) {
                b as char
            } else {
                '.'
            });
        }
    }
    out
}

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn differential_vs_naive_formatter() {
    let mut rng = 0x600d_c0de_feed_5eed_u64;
    for _ in 0..20000 {
        let len = (xorshift(&mut rng) % 40) as usize;
        let rs_choices = [16usize, 32, 8, 24, 64];
        let rs = rs_choices[(xorshift(&mut rng) % rs_choices.len() as u64) as usize];
        let gs_choices = [1usize, 2, 4, 8, 3, 5, 0, 16];
        let gs = gs_choices[(xorshift(&mut rng) % gs_choices.len() as u64) as usize];
        let ascii = xorshift(&mut rng) & 1 == 0;

        let mut buf = [FILL_CHAR; TEST_HEXDUMP_BUF_SIZE];
        hex_dump_to_buffer(&DATA_B[..len.min(DATA_B.len())], rs, gs, &mut buf, ascii);
        let got =
            core::str::from_utf8(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(0)]).unwrap();

        let want = naive_format(&DATA_B[..len.min(DATA_B.len())], rs, gs, ascii);
        assert_eq!(
            got,
            want.as_str(),
            "len={len} rs={rs} gs={gs} ascii={ascii}"
        );
    }
}

#[test]
fn unbounded_lines_never_truncate() {
    for rs in [16usize, 32] {
        for gs in [1usize, 2, 4, 8] {
            for len in 1..=rs {
                let line = hex_dump_line(&DATA_B[..len], rs, gs, true);
                assert!(!line.as_bytes().is_empty());
                assert!(line.len_is_consistent(), "internal NUL/len mismatch");
                let want = naive_format(&DATA_B[..len], rs, gs, true);
                assert_eq!(line.as_str(), want.as_str(), "len={len} rs={rs} gs={gs}");
            }
        }
    }
}

impl HexLine {
    /// Test-only sanity: stored length matches strlen of internal buffer.
    fn len_is_consistent(&self) -> bool {
        self.buf[self.len] == 0 && self.buf[..self.len].iter().all(|&b| b != 0)
    }
}

#[test]
fn multi_line_dump_matches_per_row_calls() {
    // print_hex_dump() iteration structure: chunks of rowsize, offsets by
    // cumulative position; last line may be short.
    let big: Vec<u8> = (0..100u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();

    for rs in [16usize, 32] {
        for gs in [1usize, 2, 4, 8] {
            for ascii in [false, true] {
                let lines: Vec<DumpLine> = print_hex_dump_lines(&big, rs, gs, ascii).collect();

                // Number of lines: ceil(len / rs)
                let expected_count = big.len().div_ceil(rs);
                assert_eq!(lines.len(), expected_count, "rs={rs} gs={gs}");

                for (idx, dl) in lines.iter().enumerate() {
                    assert_eq!(dl.offset, idx * rs);
                    let start = idx * rs;
                    let end = (start + rs).min(big.len());
                    let single = hex_dump_line(&big[start..end], rs, gs, ascii);
                    assert_eq!(dl.line.as_str(), single.as_str());

                    // Offset prefix formatting (%.8x: )
                    let pfx = dl.offset_prefix_bytes();
                    assert_eq!(&pfx[..8], format!("{:08x}", dl.offset).as_bytes());
                    assert_eq!(&pfx[8..], b": ");
                }
            }
        }
    }
}

#[test]
fn example_output_from_c_doc_comment() {
    // Doc comment example in lib/hexdump.c:
    // 40 41 42 ... 4f  @ABCDEFGHIJKLMNO
    let frame: Vec<u8> = (0x40u8..=0x4f).collect();
    let line = hex_dump_line(&frame, 16, 1, true);
    assert_eq!(
        line.as_str(),
        "40 41 42 43 44 45 46 47 48 49 4a 4b 4c 4d 4e 4f  @ABCDEFGHIJKLMNO"
    );

    // Multi-row dump with offset prefix, like print_hex_dump(KERN_DEBUG, "",
    // DUMP_PREFIX_OFFSET, 16, 1, ...) over 0x30..=0x57. Expected rows are
    // built independently: hex pairs joined by spaces, padded to column
    // 49, then the printable ASCII rendering.
    let data: Vec<u8> = (0x30u8..=0x57).collect();
    for dl in print_hex_dump_lines(&data, 16, 1, true) {
        assert_eq!(dl.offset % 16, 0);
        let row = &data[dl.offset..(dl.offset + 16).min(data.len())];

        let mut hex_part = String::new();
        for b in row.iter() {
            hex_part.push_str(format!("{:02x} ", b).as_str());
        }
        hex_part.pop(); // trailing space removed, like the C code
        while hex_part.len() < 49 {
            hex_part.push(' ');
        }
        for &b in row {
            hex_part.push(if (0x20..=0x7e).contains(&b) {
                b as char
            } else {
                '.'
            });
        }

        assert_eq!(dl.line.as_str(), hex_part.as_str(), "offset {}", dl.offset);
        assert_eq!(
            &dl.offset_prefix_bytes()[..8],
            format!("{:08x}", dl.offset).as_bytes()
        );
    }
}

// ---------------------------------------------------------------------------
// hex_to_bin / hex2bin / bin2hex ports
// ---------------------------------------------------------------------------

#[test]
fn hex_to_bin_full_domain() {
    for ch in 0u8..=255 {
        let want = match ch {
            b'0'..=b'9' => (ch - b'0') as i32,
            b'a'..=b'f' => (ch - b'a') as i32 + 10,
            b'A'..=b'F' => (ch - b'A') as i32 + 10,
            _ => -1,
        };
        assert_eq!(hex_to_bin(ch), want, "ch={:#04x}", ch);
    }
}

#[test]
fn hex2bin_bin2hex_roundtrip_and_errors() {
    let src: Vec<u8> = (0..=255u8).collect();
    let mut hex = [0u8; 512];
    let rest = bin2hex(&mut hex, &src).unwrap();
    assert_eq!(rest.len(), 0);
    assert_eq!(&hex[..2], b"00");
    assert_eq!(&hex[510..512], b"ff");

    let mut back = [0u8; 256];
    hex2bin(&mut back, &hex).unwrap();
    assert_eq!(&back[..], &src[..]);

    // Invalid character anywhere fails with Invalid (-EINVAL).
    let bad = *b"0g";
    let mut out = [0u8; 1];
    assert_eq!(hex2bin(&mut out, &bad), Err(HexError::Invalid));
    assert_eq!(HexError::Invalid.errno(), -22);

    // Uppercase/lowercase mix accepted.
    let mut out2 = [0u8; 4];
    hex2bin(&mut out2, b"DeAdBeEf").unwrap();
    assert_eq!(&out2, &[0xde, 0xad, 0xbe, 0xef]);

    // Short inputs: TooShort instead of C's out-of-bounds read.
    let mut three = [0u8; 3];
    assert_eq!(hex2bin(&mut three, b"001122"), Ok(()));
    assert_eq!(hex2bin(&mut three, b"0011"), Err(HexError::TooShort)); // 4 < 6 digits

    // Short dst for bin2hex: TooShort instead of C's overflow.
    let mut tiny = [0u8; 1];
    assert_eq!(bin2hex(&mut tiny, &[0xaa]), Err(HexError::TooShort));
}
