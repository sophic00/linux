// SPDX-License-Identifier: (GPL-2.0 OR MIT)
//! Rust rewrite of the Linux kernel's `lib/glob.c` (`include/linux/glob.h`).
//!
//! Shell-style pattern matching, like `!fnmatch(pat, str, 0)`. The pattern
//! must match the *entire* string. Pattern metacharacters are `?`, `*`,
//! `[` and `\` (and, inside character classes, `!`, `-` and `]`).
//!
//! Semantics preserved from the C implementation:
//! - `*` matches any run of characters, including `/` and the empty string;
//!   there is no special treatment of `/` or leading `.`.
//! - Character classes are complemented by a leading `!` (glob(7) style);
//!   regex-style `[^a-z]` is NOT supported — `^` inside a class is literal.
//! - The first span of a class may begin with `]`.
//! - A `-` forms a range unless immediately followed by `]` (then `-` is a
//!   literal span character).
//! - An opening bracket without a matching close (or otherwise malformed,
//!   e.g. a range whose upper bound is missing) is matched literally: the
//!   `[` must then appear in the string, and matching continues with the
//!   bytes after the `[` treated as ordinary pattern characters.
//! - A trailing lone `\` in the pattern escapes the (virtual) terminator,
//!   so it can only match at end-of-string — same as C.
//! - Runtime is at most quadratic in strlen(str)*strlen(pat); the matcher
//!   is iterative with single-level backtracking to the last `*`, exactly
//!   like the C code (see glob_match_str()).
//!
//! Deviations from C (noted for review):
//! - The API takes Rust `&str`s instead of NUL-terminated pointers. To keep
//!   observable behavior identical to C, both inputs are logically truncated
//!   at their first NUL byte, mirroring C string termination.
//! - [`glob_match_len`] takes a byte buffer plus length, mirroring the C
//!   function of the same name (used for non-NUL-terminated buffers).

#![no_std]
#![deny(unsafe_code)]

/// Byte at `i`, or 0 when past the end — mirrors reading through the NUL
/// terminator of a C string.
#[inline]
fn at(b: &[u8], i: usize) -> u8 {
    if i < b.len() {
        b[i]
    } else {
        0
    }
}

/// Logical truncation at the first NUL byte (C string semantics).
fn until_nul(b: &[u8]) -> &[u8] {
    match b.iter().position(|&x| x == 0) {
        Some(i) => &b[..i],
        None => b,
    }
}

/// Outcome of scanning one `[...]` class against character `c`.
enum ClassScan {
    /// Class accepted `c`; payload is the pattern index just past `]`.
    Accepted(usize),
    /// Well-formed class that did not accept `c` (incl. negated accept):
    /// proceed to backtracking.
    Rejected,
    /// Malformed class (hit end-of-string / missing range bound): the `[`
    /// is treated as a literal character instead, per the C `goto literal`.
    Malformed,
}

/// Scan a character class starting at `p[ci]` (the byte after `[`),
/// mirroring the do/while span loop of `glob_match_str()`.
fn scan_class(p: &[u8], mut ci: usize, c: u8) -> ClassScan {
    let inverted = at(p, ci) == b'!';
    if inverted {
        ci += 1;
    }
    let mut matched = false;

    /*
     * Iterate over each span in the character class.
     * A span is either a single character a, or a range a-b.
     * The first span may begin with ']'.
     */
    let mut a = at(p, ci);
    ci += 1;
    loop {
        if a == 0 {
            // Malformed.
            return ClassScan::Malformed;
        }
        let mut b = a;

        if at(p, ci) == b'-' && at(p, ci + 1) != b']' {
            b = at(p, ci + 1);
            if b == 0 {
                // Malformed (missing upper bound).
                return ClassScan::Malformed;
            }
            ci += 2;
            // Any special action if a > b? (None, per C.)
        }
        if a <= c && c <= b {
            matched = true;
        }

        // while ((a = *class++) != ']')
        a = at(p, ci);
        ci += 1;
        if a == b']' {
            return if matched == inverted {
                ClassScan::Rejected
            } else {
                ClassScan::Accepted(ci)
            };
        }
    }
}

/// `glob_match()`: shell-style glob matching against a whole string.
///
/// Returns true if `pat` matches all of `str`. Equivalent to
/// `!fnmatch(pat, str, 0)`.
pub fn glob_match(pat: &str, string: &str) -> bool {
    let pat = until_nul(pat.as_bytes());
    let string = until_nul(string.as_bytes());
    matches(pat, string)
}

/// `glob_match_len()`: glob match against a length-bounded byte buffer.
///
/// Like [`glob_match`], but only the first `len` bytes of `string` are read,
/// so the buffer need not be NUL-terminated. A NUL byte within `len` still
/// terminates the matched portion, exactly as in C.
pub fn glob_match_len(pat: &str, string: &[u8], len: usize) -> bool {
    let pat = until_nul(pat.as_bytes());
    let string = until_nul(&string[..len.min(string.len())]);
    matches(pat, string)
}

/// The core iterative matcher: a direct translation of `glob_match_str()`.
fn matches(p: &[u8], s: &[u8]) -> bool {
    /*
     * Backtrack to previous * on mismatch and retry starting one
     * character later in the string. Because * matches all characters
     * (no exception for /), it can be easily proved that there's
     * never a need to backtrack multiple levels.
     */
    let mut back_pat: Option<usize> = None;
    let mut back_str: usize = 0;

    let mut pi = 0usize; // index into p
    let mut si = 0usize; // index into s

    /*
     * Loop over each token (character or class) in pat, matching
     * it against the remaining unmatched tail of str. Return false
     * on mismatch, or true after matching the trailing nul bytes.
     */
    loop {
        let c = at(s, si); // current string byte, '\0' when exhausted

        enum Tok {
            Question,
            Star,
            Class,
            Literal(u8),
        }

        let d = at(p, pi);
        let tok = match d {
            b'?' => Tok::Question,
            b'*' => Tok::Star,
            b'[' => Tok::Class,
            b'\\' => {
                // Escape: take the next pattern byte verbatim (which may be
                // the virtual terminator, exactly as in C).
                Tok::Literal(at(p, pi + 1))
            }
            _ => Tok::Literal(d),
        };

        // Advance both cursors past the consumed token bytes (C does this
        // once per loop iteration, before the switch).
        si += 1;
        pi += match tok {
            Tok::Literal(_) if d == b'\\' => 2,
            _ => 1,
        };

        // Shared tail: retry from the last '*', one character later in str.
        macro_rules! backtrack {
            () => {{
                match back_pat {
                    None => return false, // No point continuing.
                    Some(bp) => {
                        back_str += 1;
                        pi = bp;
                        si = back_str;
                        continue;
                    }
                }
            }};
        }

        match tok {
            Tok::Question => {
                // Wildcard: anything but nul.
                if c == 0 {
                    return false;
                }
            }
            Tok::Star => {
                // Any-length wildcard.
                if at(p, pi) == 0 {
                    // Optimize trailing * case.
                    return true;
                }
                back_pat = Some(pi);
                si -= 1; // Allow zero-length match.
                back_str = si;
            }
            Tok::Class => {
                // Character class.
                if c == 0 {
                    // No possible match.
                    return false;
                }
                match scan_class(p, pi, c) {
                    ClassScan::Accepted(next_pi) => pi = next_pi,
                    ClassScan::Rejected => {
                        // Class did not accept c: backtrack.
                        if c == 0 || back_pat.is_none() {
                            return false;
                        }
                        backtrack!();
                    }
                    ClassScan::Malformed => {
                        // C's `goto literal`: compare against '[' itself,
                        // then fall through to the normal mismatch logic.
                        if c == d {
                            continue;
                        }
                        if c == 0 {
                            return false;
                        }
                        backtrack!();
                    }
                }
            }
            Tok::Literal(dd) => {
                // Literal character (possibly via escape).
                if c == dd {
                    if dd == 0 {
                        // Matched the trailing nul bytes: full match.
                        return true;
                    }
                    continue;
                }
                if c == 0 || back_pat.is_none() {
                    return false; // No point continuing.
                }
                backtrack!();
            }
        }
    }
}

#[cfg(test)]
mod tests;
