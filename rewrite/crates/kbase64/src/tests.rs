//! Tests ported from the kernel's `lib/tests/base64_kunit.c`, plus
//! round-trip property tests over random buffers using `krand`.
//!
//! The KUnit benchmark cases are not ported (they measure timing).

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;

use alloc::{string::String, vec, vec::Vec};

use krand::{Krand, Rng};

use super::*;

/// Convenience wrapper mirroring the KUnit `expect_encode_ok` helper:
/// encode into a sufficiently large buffer and compare against `expected`.
fn expect_encode_ok(src: &[u8], expected: &str, padding: bool, variant: Base64Variant) {
    let mut buf = [0u8; 128];
    let n = encode(src, &mut buf, padding, variant).expect("encode failed");
    assert_eq!(n, expected.len(), "encode({src:?}) length");
    assert_eq!(&buf[..n], expected.as_bytes(), "encode({src:?}) bytes");
}

/// Mirrors the KUnit `expect_decode_ok` helper.
fn expect_decode_ok(src: &[u8], expected: &[u8], padding: bool, variant: Base64Variant) {
    let mut buf = [0u8; 128];
    let n = decode(src, &mut buf, padding, variant).expect("decode failed");
    assert_eq!(n, expected.len(), "decode({src:?}) length");
    assert_eq!(&buf[..n], expected, "decode({src:?}) bytes");
}

/// Mirrors the KUnit `expect_decode_err` helper.
fn expect_decode_err(src: &[u8], padding: bool, variant: Base64Variant) {
    let mut buf = [0u8; 64];
    assert_eq!(
        decode(src, &mut buf, padding, variant),
        Err(Error::Invalid),
        "decode({src:?}) should fail"
    );
}

#[test]
fn base64_std_encode_tests() {
    // With padding
    expect_encode_ok(b"", "", true, Base64Variant::Std);
    expect_encode_ok(b"f", "Zg==", true, Base64Variant::Std);
    expect_encode_ok(b"fo", "Zm8=", true, Base64Variant::Std);
    expect_encode_ok(b"foo", "Zm9v", true, Base64Variant::Std);
    expect_encode_ok(b"foob", "Zm9vYg==", true, Base64Variant::Std);
    expect_encode_ok(b"fooba", "Zm9vYmE=", true, Base64Variant::Std);
    expect_encode_ok(b"foobar", "Zm9vYmFy", true, Base64Variant::Std);

    // Extra cases with padding
    expect_encode_ok(
        b"Hello, world!",
        "SGVsbG8sIHdvcmxkIQ==",
        true,
        Base64Variant::Std,
    );
    expect_encode_ok(
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=",
        true,
        Base64Variant::Std,
    );
    expect_encode_ok(
        b"abcdefghijklmnopqrstuvwxyz",
        "YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXo=",
        true,
        Base64Variant::Std,
    );
    expect_encode_ok(
        b"0123456789+/",
        "MDEyMzQ1Njc4OSsv",
        true,
        Base64Variant::Std,
    );

    // Without padding
    expect_encode_ok(b"", "", false, Base64Variant::Std);
    expect_encode_ok(b"f", "Zg", false, Base64Variant::Std);
    expect_encode_ok(b"fo", "Zm8", false, Base64Variant::Std);
    expect_encode_ok(b"foo", "Zm9v", false, Base64Variant::Std);
    expect_encode_ok(b"foob", "Zm9vYg", false, Base64Variant::Std);
    expect_encode_ok(b"fooba", "Zm9vYmE", false, Base64Variant::Std);
    expect_encode_ok(b"foobar", "Zm9vYmFy", false, Base64Variant::Std);

    // Extra cases without padding
    expect_encode_ok(
        b"Hello, world!",
        "SGVsbG8sIHdvcmxkIQ",
        false,
        Base64Variant::Std,
    );
    expect_encode_ok(
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo",
        false,
        Base64Variant::Std,
    );
    expect_encode_ok(
        b"abcdefghijklmnopqrstuvwxyz",
        "YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXo",
        false,
        Base64Variant::Std,
    );
    expect_encode_ok(
        b"0123456789+/",
        "MDEyMzQ1Njc4OSsv",
        false,
        Base64Variant::Std,
    );
}

#[test]
fn base64_std_decode_tests() {
    // -------- With padding --------
    expect_decode_ok(b"", b"", true, Base64Variant::Std);
    expect_decode_ok(b"Zg==", b"f", true, Base64Variant::Std);
    expect_decode_ok(b"Zm8=", b"fo", true, Base64Variant::Std);
    expect_decode_ok(b"Zm9v", b"foo", true, Base64Variant::Std);
    expect_decode_ok(b"Zm9vYg==", b"foob", true, Base64Variant::Std);
    expect_decode_ok(b"Zm9vYmE=", b"fooba", true, Base64Variant::Std);
    expect_decode_ok(b"Zm9vYmFy", b"foobar", true, Base64Variant::Std);
    expect_decode_ok(
        b"SGVsbG8sIHdvcmxkIQ==",
        b"Hello, world!",
        true,
        Base64Variant::Std,
    );
    expect_decode_ok(
        b"QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=",
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        true,
        Base64Variant::Std,
    );
    expect_decode_ok(
        b"YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXo=",
        b"abcdefghijklmnopqrstuvwxyz",
        true,
        Base64Variant::Std,
    );

    // Error cases
    expect_decode_err(b"Zg=!", true, Base64Variant::Std);
    expect_decode_err(b"Zm$=", true, Base64Variant::Std);
    expect_decode_err(b"Z===", true, Base64Variant::Std);
    expect_decode_err(b"Zg", true, Base64Variant::Std);
    expect_decode_err(b"Zm9v====", true, Base64Variant::Std);
    expect_decode_err(b"Zm==A", true, Base64Variant::Std);

    let with_nul: [u8; 4] = [b'Z', b'g', 0, b'='];
    expect_decode_err(&with_nul, true, Base64Variant::Std);

    // -------- Without padding --------
    expect_decode_ok(b"", b"", false, Base64Variant::Std);
    expect_decode_ok(b"Zg", b"f", false, Base64Variant::Std);
    expect_decode_ok(b"Zm8", b"fo", false, Base64Variant::Std);
    expect_decode_ok(b"Zm9v", b"foo", false, Base64Variant::Std);
    expect_decode_ok(b"Zm9vYg", b"foob", false, Base64Variant::Std);
    expect_decode_ok(b"Zm9vYmE", b"fooba", false, Base64Variant::Std);
    expect_decode_ok(b"Zm9vYmFy", b"foobar", false, Base64Variant::Std);
    expect_decode_ok(b"TWFu", b"Man", false, Base64Variant::Std);
    expect_decode_ok(
        b"SGVsbG8sIHdvcmxkIQ",
        b"Hello, world!",
        false,
        Base64Variant::Std,
    );
    expect_decode_ok(
        b"QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo",
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        false,
        Base64Variant::Std,
    );
    expect_decode_ok(
        b"YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXo",
        b"abcdefghijklmnopqrstuvwxyz",
        false,
        Base64Variant::Std,
    );
    expect_decode_ok(
        b"MDEyMzQ1Njc4OSsv",
        b"0123456789+/",
        false,
        Base64Variant::Std,
    );

    // Error cases
    expect_decode_err(b"Zg=!", false, Base64Variant::Std);
    expect_decode_err(b"Zm$=", false, Base64Variant::Std);
    expect_decode_err(b"Z===", false, Base64Variant::Std);
    expect_decode_err(b"Zg=", false, Base64Variant::Std);
    expect_decode_err(b"Zm9v====", false, Base64Variant::Std);
    expect_decode_err(b"Zm==v", false, Base64Variant::Std);

    let with_nul: [u8; 4] = [b'Z', b'g', 0, b'='];
    expect_decode_err(&with_nul, false, Base64Variant::Std);
}

#[test]
fn base64_variant_tests() {
    let sample1: [u8; 5] = [0x00, 0xfb, 0xff, 0x7f, 0x80];

    // URLSAFE: identical to STD except '-'/'_' replace '+'/'/'.
    let mut std_buf = [0u8; 128];
    let mut url_buf = [0u8; 128];
    let n_std = encode(&sample1, &mut std_buf, false, Base64Variant::Std).unwrap();
    let n_url = encode(&sample1, &mut url_buf, false, Base64Variant::Urlsafe).unwrap();
    assert_eq!(n_std, n_url);
    for i in 0..n_std {
        let expected = match std_buf[i] {
            b'+' => b'-',
            b'/' => b'_',
            other => other,
        };
        assert_eq!(url_buf[i], expected);
    }
    let mut back = [0u8; 128];
    let m = decode(&url_buf[..n_url], &mut back, false, Base64Variant::Urlsafe).unwrap();
    assert_eq!(m, sample1.len());
    assert_eq!(&back[..m], &sample1);

    // IMAP: identical to STD except ',' replaces '/'.
    let mut imap_buf = [0u8; 128];
    let n_std = encode(&sample1, &mut std_buf, false, Base64Variant::Std).unwrap();
    let n_imap = encode(&sample1, &mut imap_buf, false, Base64Variant::Imap).unwrap();
    assert_eq!(n_std, n_imap);
    for i in 0..n_std {
        let expected = if std_buf[i] == b'/' { b',' } else { std_buf[i] };
        assert_eq!(imap_buf[i], expected);
    }
    let m = decode(&imap_buf[..n_imap], &mut back, false, Base64Variant::Imap).unwrap();
    assert_eq!(m, sample1.len());
    assert_eq!(&back[..m], &sample1);

    // '=' is only valid in the STD alphabet when padding is disabled:
    // decoding "Zg==" fails for both other variants.
    let bad = b"Zg==";
    let mut tmp = [0u8; 8];
    assert_eq!(
        decode(bad, &mut tmp, false, Base64Variant::Urlsafe),
        Err(Error::Invalid)
    );
    assert_eq!(
        decode(bad, &mut tmp, false, Base64Variant::Imap),
        Err(Error::Invalid)
    );
}

#[test]
fn undersized_dst_is_rejected() {
    // Rust-side deviation from C: no silent overflow.
    let mut small = [0u8; 3];
    assert_eq!(
        encode(b"fooba", &mut small, true, Base64Variant::Std),
        Err(Error::Invalid)
    );
    assert_eq!(
        encode(b"fooba", &mut small, false, Base64Variant::Std),
        Err(Error::Invalid)
    );
    // Exactly-sized buffer works.
    let mut exact = [0u8; 8];
    assert_eq!(
        encode(b"fooba", &mut exact, true, Base64Variant::Std),
        Ok(8)
    );

    let mut tiny = [0u8; 1];
    assert_eq!(
        decode(b"Zm9vYmFy", &mut tiny, false, Base64Variant::Std),
        Err(Error::Invalid)
    );
}

#[test]
fn base64_chars_matches_c_macro() {
    // DIV_ROUND_UP(nbytes * 4, 3): conservative upper bound.
    assert_eq!(base64_chars(0), 0);
    assert_eq!(base64_chars(1), 2); // ceil(4/3) = 2
    assert_eq!(base64_chars(2), 3);
    assert_eq!(base64_chars(3), 4);
    assert_eq!(base64_chars(6), 8);
    assert_eq!(base64_chars(13), 18);
}

/// Independent naive reference encoder written directly from RFC 4648
/// (bit-accumulation style), used as a differential oracle.
fn naive_encode(src: &[u8], table: &[u8; 64], padding: bool) -> String {
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = String::new();
    for &b in src {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            out.push(table[(acc >> bits) as usize & 0x3f] as char);
        }
    }
    if bits > 0 {
        out.push(table[(acc << (6 - bits)) as usize & 0x3f] as char);
    }
    if padding {
        while out.len() % 4 != 0 {
            out.push('=');
        }
    }
    out
}

#[test]
fn roundtrip_property_all_variants() {
    const TABLES: [&[u8; 64]; 3] = [
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,",
    ];
    let variants = [
        (Base64Variant::Std, 0usize),
        (Base64Variant::Urlsafe, 1),
        (Base64Variant::Imap, 2),
    ];

    let mut rng = Krand::seed_from_u64(0x1234_5678_9abc_def0);

    for len in 0..300usize {
        let mut src = vec![0u8; len];
        rng.fill_bytes(&mut src);

        for &(variant, ti) in &variants {
            for &padding in &[true, false] {
                let mut enc = vec![0u8; len.div_ceil(3) * 4]; // exact worst case (BASE64_CHARS underestimates when padding)
                let enc_len =
                    encode(&src, &mut enc, padding, variant).expect("encode with ample dst");

                // Differential: must match the independent RFC encoder exactly.
                let want = naive_encode(&src, TABLES[ti], padding);
                assert_eq!(
                    &enc[..enc_len],
                    want.as_bytes(),
                    "len {len} v{ti} pad {padding}"
                );

                // Round-trip: decode must reproduce the original.
                let mut dec = vec![0u8; enc_len.div_ceil(4) * 3]; // matches decode() capacity contract
                let dec_len = decode(&enc[..enc_len], &mut dec, padding, variant)
                    .expect("decode of own encoding");
                assert_eq!(dec_len, len, "len {len} v{ti} pad {padding}");
                assert_eq!(&dec[..dec_len], &src, "len {len} v{ti} pad {padding}");
            }
        }
    }
}

#[test]
fn roundtrip_random_large_buffers() {
    let mut rng = Krand::seed_from_u64(0xdead_beef_cafe_f00d);
    for _ in 0..50 {
        let len = rng.below(8192) as usize;
        let mut src = vec![0u8; len];
        rng.fill_bytes(&mut src);
        for &variant in &[
            Base64Variant::Std,
            Base64Variant::Urlsafe,
            Base64Variant::Imap,
        ] {
            for &padding in &[true, false] {
                let mut enc = vec![0u8; len.div_ceil(3) * 4]; // exact worst case (BASE64_CHARS underestimates when padding)
                let enc_len = encode(&src, &mut enc, padding, variant).unwrap();
                let mut dec = vec![0u8; enc_len.div_ceil(4) * 3]; // matches decode() capacity contract
                let dec_len = decode(&enc[..enc_len], &mut dec, padding, variant).unwrap();
                assert_eq!(dec_len, len);
                assert_eq!(&dec[..dec_len], &src);
            }
        }
    }
}

#[test]
fn exhaustive_roundtrip_vs_rfc_oracle() {
    const TABLES: [&[u8; 64]; 3] = [
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,",
    ];
    fn naive(src: &[u8], table: &[u8; 64], padding: bool) -> Vec<u8> {
        let mut acc: u32 = 0;
        let mut bits = 0u32;
        let mut out = Vec::new();
        for &b in src {
            acc = (acc << 8) | b as u32;
            bits += 8;
            while bits >= 6 {
                bits -= 6;
                out.push(table[((acc >> bits) & 0x3f) as usize]);
            }
        }
        if bits > 0 {
            out.push(table[((acc << (6 - bits)) & 0x3f) as usize]);
        }
        if padding {
            while out.len() % 4 != 0 {
                out.push(b'=');
            }
        }
        out
    }
    let variants = [
        (Base64Variant::Std, 0usize),
        (Base64Variant::Urlsafe, 1),
        (Base64Variant::Imap, 2),
    ];
    for len in 0..1200usize {
        let mut r2 = Krand::seed_from_u64(0xfeed_face);
        let mut src = vec![0u8; len];
        r2.fill_bytes(&mut src);
        for &(variant, ti) in &variants {
            let table: &[u8; 64] = TABLES[ti];
            for &padding in &[true, false] {
                let want = naive(&src, table, padding);
                let mut enc = vec![0u8; len.div_ceil(3) * 4];
                let enc_len = encode(&src, &mut enc, padding, variant).unwrap();
                assert_eq!(
                    &enc[..enc_len],
                    &want[..],
                    "ENC mismatch len={len} v={ti} pad={padding}"
                );
                let mut dec = vec![0u8; enc_len.div_ceil(4) * 3 + 3];
                let dec_len = decode(&want, &mut dec, padding, variant).unwrap_or_else(|_| {
                    panic!("DEC err len={len} v={ti} pad={padding}");
                });
                if dec_len != len || dec[..dec_len] != src[..] {
                    panic!("RT mismatch len={len} v={ti} pad={padding} dec_len={dec_len}");
                }
            }
        }
    }
}
