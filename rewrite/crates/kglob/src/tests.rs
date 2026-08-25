//! Tests for the `lib/glob.c` rewrite: cases ported verbatim from the
//! kernel's `lib/tests/glob_kunit.c`, plus exhaustive and randomized
//! differential testing against an independent brute-force matcher.

// SPDX-License-Identifier: (GPL-2.0 OR MIT)

extern crate alloc;

use alloc::{vec, vec::Vec};

use super::*;

/// The ported `glob_test_cases[]` table from lib/tests/glob_kunit.c.
const KUNIT_CASES: &[(&str, &str, bool)] = &[
    /* Some basic tests */
    ("a", "a", true),
    ("a", "b", false),
    ("a", "aa", false),
    ("a", "", false),
    ("", "", true),
    ("", "a", false),
    /* Simple character class tests */
    ("[a]", "a", true),
    ("[a]", "b", false),
    ("[!a]", "a", false),
    ("[!a]", "b", true),
    ("[ab]", "a", true),
    ("[ab]", "b", true),
    ("[ab]", "c", false),
    ("[!ab]", "c", true),
    ("[a-c]", "b", true),
    ("[a-c]", "d", false),
    /* Corner cases in character class parsing */
    ("[a-c-e-g]", "-", true),
    ("[a-c-e-g]", "d", false),
    ("[a-c-e-g]", "f", true),
    ("[]a-ceg-ik[]", "a", true),
    ("[]a-ceg-ik[]", "]", true),
    ("[]a-ceg-ik[]", "[", true),
    ("[]a-ceg-ik[]", "h", true),
    ("[]a-ceg-ik[]", "f", false),
    ("[!]a-ceg-ik[]", "h", false),
    ("[!]a-ceg-ik[]", "]", false),
    ("[!]a-ceg-ik[]", "f", true),
    /* Simple wild cards */
    ("?", "a", true),
    ("?", "aa", false),
    ("??", "a", false),
    ("?x?", "axb", true),
    ("?x?", "abx", false),
    ("?x?", "xab", false),
    /* Asterisk wild cards (backtracking) */
    ("*??", "a", false),
    ("*??", "ab", true),
    ("*??", "abc", true),
    ("*??", "abcd", true),
    ("??*", "a", false),
    ("??*", "ab", true),
    ("??*", "abc", true),
    ("??*", "abcd", true),
    ("?*?", "a", false),
    ("?*?", "ab", true),
    ("?*?", "abc", true),
    ("?*?", "abcd", true),
    ("*b", "b", true),
    ("*b", "ab", true),
    ("*b", "ba", false),
    ("*b", "bb", true),
    ("*b", "abb", true),
    ("*b", "bab", true),
    ("*bc", "abbc", true),
    ("*bc", "bc", true),
    ("*bc", "bbc", true),
    ("*bc", "bcbc", true),
    /* Multiple asterisks (complex backtracking) */
    ("*ac*", "abacadaeafag", true),
    ("*ac*ae*ag*", "abacadaeafag", true),
    ("*a*b*[bc]*[ef]*g*", "abacadaeafag", true),
    ("*a*b*[ef]*[cd]*g*", "abacadaeafag", false),
    ("*abcd*", "abcabcabcabcdefg", true),
    ("*ab*cd*", "abcabcabcabcdefg", true),
    ("*abcd*abcdef*", "abcabcdabcdeabcdefg", true),
    ("*abcd*", "abcabcabcabcefg", false),
    ("*ab*cd*", "abcabcabcabcefg", false),
];

#[test]
fn kunit_cases() {
    for &(pat, s, expected) in KUNIT_CASES {
        assert_eq!(
            glob_match(pat, s),
            expected,
            "Pattern: {pat:?}, String: {s:?}, Expected: {expected}"
        );
    }
}

/// Extra cases covering quirks of the C implementation that the KUnit table
/// does not exercise directly.
#[test]
fn c_quirk_cases() {
    // Unclosed bracket: matched literally.
    assert!(glob_match("[", "["));
    assert!(glob_match("[a", "[a"));
    assert!(!glob_match("[a", "a"));
    assert!(glob_match("*[x", "ab[x")); // literal fallback after '*'
    // Range with missing upper bound: malformed -> literal '['.
    assert!(glob_match("[a-", "[a-"));
    assert!(!glob_match("[a-", "[a"));
    // '-' right before ']' is a literal span member.
    assert!(glob_match("[a-]", "-"));
    assert!(glob_match("[a-]", "a"));
    // '^' is NOT special (unlike regex).
    assert!(glob_match("[^a]", "^"));
    assert!(!glob_match("[^a]", "b"));
    // Trailing lone backslash escapes the terminator: only matches EOS.
    assert!(glob_match("abc\\", "abc"));
    assert!(!glob_match("abc\\", "abc\\"));
    // Escaped metacharacters.
    assert!(glob_match("a\\*b", "a*b"));
    assert!(!glob_match("a\\*b", "axb"));
    assert!(glob_match("a\\?b", "a?b"));
    assert!(!glob_match("a\\?b", "axb"));
    assert!(glob_match("\\[a\\]", "[a]"));
    // Escaped backslash.
    assert!(glob_match("a\\\\b", "a\\b"));
    // '*' optimizes the trailing-* case even after failures.
    assert!(glob_match("*aaaaa", "aaaaaaaaaa")); // documented worst case
    // Empty pattern / empty string edge interplay with '*'.
    assert!(glob_match("*", ""));
    assert!(glob_match("**", ""));
    assert!(glob_match("*", "anything"));
}

/// NUL handling mirrors C string termination.
#[test]
fn nul_termination_semantics() {
    // Embedded NUL acts as end-of-string, like C.
    assert!(glob_match("a", "a\u{0}b"));
    assert!(!glob_match("ab", "a\u{0}b"));
    // Pattern NUL truncation.
    assert!(glob_match("a\u{0}*", "a"));
}

/// glob_match_len(): bounded buffers need not be NUL-terminated.
#[test]
fn match_len_cases() {
    assert!(glob_match_len("abc", b"abcdef", 3));
    // "abcd" DOES match within len=4 (window is exactly "abcd");
    // it fails only with a shorter window.
    assert!(glob_match_len("abcd", b"abcdef", 4));
    assert!(!glob_match_len("abcd", b"abcdef", 3));
    assert!(!glob_match_len("abc", b"abcdef", 2));
    assert!(glob_match_len("ab*", b"abXYZ!@#", 8));
    // NUL within len still terminates.
    assert!(glob_match_len("ab*", b"ab\0XYZ!", 7));
    assert!(!glob_match_len("ab*z", b"ab\0XYZz", 7));
    // len longer than buffer is clamped safely.
    assert!(glob_match_len("ab", b"ab", 100));
    // Empty window.
    assert!(glob_match_len("", b"abc", 0));
    assert!(!glob_match_len("a", b"abc", 0));
}

// ---------------------------------------------------------------------------
// Independent reference matcher (brute-force recursion). Written from the
// documented glob(7)/fnmatch semantics and the C code's quirk list, NOT a
// structural copy of the iterative implementation above.
// ---------------------------------------------------------------------------

enum RefClass {
    /// Accepted; payload = offset just past ']'.
    Yes(usize),
    /// Well-formed but rejected.
    No,
    /// Malformed: '[' falls back to a literal character.
    Malformed,
}

fn ref_class(class: &[u8], c: u8) -> RefClass {
    let mut i = if class.first() == Some(&b'!') { 1 } else { 0 };
    let inverted = i == 1;
    let mut hit = false;

    // First span character may be ']'.
    let mut a = if i < class.len() { class[i] } else { 0 };
    i += 1;

    loop {
        if a == 0 {
            return RefClass::Malformed;
        }
        let mut b = a;
        if i < class.len() && class[i] == b'-' {
            if i + 1 >= class.len() {
                // Missing upper bound ('\0'): malformed, like C.
                return RefClass::Malformed;
            }
            if class[i + 1] != b']' {
                b = class[i + 1];
                i += 2;
            }
        }
        if a <= c && c <= b {
            hit = true;
        }
        // Read the next span start; ']' closes the class.
        if i >= class.len() {
            // Never saw the closing ']'.
            return RefClass::Malformed;
        }
        a = class[i];
        i += 1;
        if a == b']' {
            break;
        }
    }

    if hit == inverted {
        RefClass::No
    } else {
        RefClass::Yes(i)
    }
}

/// Brute-force recursive glob matcher used as the differential oracle.
fn ref_match(p: &[u8], s: &[u8]) -> bool {
    if p.is_empty() {
        return s.is_empty();
    }
    let rest_p = &p[1..];
    match p[0] {
        b'*' => (0..=s.len()).any(|i| ref_match(rest_p, &s[i..])),
        b'?' => !s.is_empty() && ref_match(rest_p, &s[1..]),
        b'[' => {
            if s.is_empty() {
                return false;
            }
            match ref_class(rest_p, s[0]) {
                RefClass::Yes(consumed) => ref_match(&rest_p[consumed..], &s[1..]),
                // A rejected class ends this branch; any enclosing '*'
                // already enumerates retries at later positions, so plain
                // failure is equivalent to the C single-level backtracking.
                RefClass::No => false,
                RefClass::Malformed => s[0] == b'[' && ref_match(rest_p, &s[1..]),
            }
        }
        b'\\' => {
            if rest_p.is_empty() {
                // Escaped terminator: matches only end-of-string (C quirk).
                return s.is_empty();
            }
            !s.is_empty() && s[0] == rest_p[0] && ref_match(&rest_p[1..], &s[1..])
        }
        ch => !s.is_empty() && s[0] == ch && ref_match(rest_p, &s[1..]),
    }
}

/// Exhaustive differential test over short patterns/strings.
#[test]
fn exhaustive_differential() {
    const PAT_ALPHABET: &[u8] = b"ab*?[]!-\\";
    const STR_ALPHABET: &[u8] = b"ab";

    let mut checked = 0usize;

    // All patterns of length 0..=3 exhaustively...
    let mut pats: Vec<Vec<u8>> = vec![Vec::new()];
    for _ in 0..3 {
        let mut next: Vec<Vec<u8>> = Vec::new();
        for p in &pats {
            for &ch in PAT_ALPHABET {
                let mut q = p.clone();
                q.push(ch);
                next.push(q);
            }
        }
        pats.extend(next);
    }
    for pat in &pats {
        check_pattern(pat, STR_ALPHABET, 0..=4, &mut checked);
    }

    fn check_pattern(pat: &[u8], str_alphabet: &[u8], str_len: core::ops::RangeInclusive<usize>, checked: &mut usize) {
        // All strings of each length exhaustively.
        fn gen_strings(alphabet: &[u8], len: usize, prefix: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
            if prefix.len() == len {
                out.push(prefix.clone());
                return;
            }
            for &ch in alphabet {
                prefix.push(ch);
                gen_strings(alphabet, len, prefix, out);
                prefix.pop();
            }
        }
        let mut strs: Vec<Vec<u8>> = vec![Vec::new()];
        for len in 1..=*str_len.end() {
            let mut pre = Vec::new();
            gen_strings(str_alphabet, len, &mut pre, &mut strs);
        }
        let pat_str = core::str::from_utf8(pat).unwrap();
        for s in &strs {
            let s_str = core::str::from_utf8(s).unwrap();
            assert_eq!(
                super::matches(pat, s),
                ref_match(pat, s),
                "pat={:?} str={:?}",
                pat_str,
                s_str
            );
            assert_eq!(
                glob_match(pat_str, s_str),
                ref_match(pat, s),
                "public api: pat={:?} str={:?}",
                pat_str,
                s_str
            );
            *checked += 1;
        }
    }

    assert!(checked > 20_000, "only checked {} pairs", checked);
}

/// Randomized differential test with longer inputs, including escapes,
/// classes, and multiple stars.
#[test]
fn randomized_differential() {
    const PAT_ALPHABET: &[u8] = b"ab*?[]!-\\";
    const STR_ALPHABET: &[u8] = b"abc";

    let mut state = 0xc0ffee_u64;
    let mut rnd = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for _ in 0..20_000 {
        let plen = (rnd() % 12) as usize;
        let slen = (rnd() % 12) as usize;
        let pat: Vec<u8> = (0..plen).map(|_| PAT_ALPHABET[(rnd() as usize) % PAT_ALPHABET.len()]).collect();
        let s: Vec<u8> = (0..slen).map(|_| STR_ALPHABET[(rnd() as usize) % STR_ALPHABET.len()]).collect();
        assert_eq!(
            matches(&pat, &s),
            ref_match(&pat, &s),
            "pat={:?} str={:?}",
            core::str::from_utf8(&pat),
            core::str::from_utf8(&s)
        );
    }
}
