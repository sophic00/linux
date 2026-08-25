//! Tests for the `lib/bitmap.c` + `lib/find_bit.c` rewrite.
//!
//! Three layers, per the program's faithfulness rules:
//! 1. Ports of key cases from the kernel's own `lib/test_bitmap.c`.
//! 2. Exhaustive differential testing of every operation against naive
//!    per-bit reference implementations over widths 0..=131 (multi-word and
//!    partial-last-word edges) with deterministic pattern families, plus
//!    full pairwise pattern enumeration for tiny widths.
//! 3. A stateful model check: random operation sequences applied to both the
//!    real `Bitmap` and the naive model, snapshots compared after each step.

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use super::*;

// ---------------------------------------------------------------------------
// Naive reference implementations (independent oracle).
// ---------------------------------------------------------------------------

/// A bitmap as a plain Vec<bool>: obviously-correct semantics.
struct Naive(Vec<bool>);

impl Naive {
    fn new(nbits: usize) -> Self {
        Naive(vec![false; nbits])
    }
    fn from_words(words: &[u64], nbits: usize) -> Self {
        let mut v = Vec::new();
        for i in 0..nbits {
            v.push(words[i / 64] & (1u64 << (i % 64)) != 0);
        }
        Naive(v)
    }
    fn weight(&self) -> usize {
        self.0.iter().filter(|&&b| b).count()
    }
    fn find_first(&self) -> usize {
        self.0.iter().position(|&b| b).unwrap_or(self.0.len())
    }
    fn find_first_zero(&self) -> usize {
        self.0.iter().position(|&b| !b).unwrap_or(self.0.len())
    }
    fn find_next(&self, start: usize) -> usize {
        if start >= self.0.len() {
            return self.0.len();
        }
        self.find_from(start, true)
    }
    fn find_next_zero(&self, start: usize) -> usize {
        if start >= self.0.len() {
            return self.0.len();
        }
        self.find_from(start, false)
    }
    fn find_from(&self, start: usize, want: bool) -> usize {
        (start..self.0.len()).find(|&i| self.0[i] == want).unwrap_or(self.0.len())
    }
    fn find_last(&self) -> usize {
        self.0.iter().rposition(|&b| b).unwrap_or(self.0.len())
    }
    fn find_nth(&self, n: usize) -> usize {
        self.0.iter().enumerate().filter(|(_, &b)| b).nth(n).map_or(self.0.len(), |(i, _)| i)
    }
}

fn words_of(n: &Naive) -> Vec<u64> {
    // Words WITH garbage-free padding (matches Bitmap invariant).
    let mut words = vec![0u64; bits_to_longs(n.0.len())];
    for (i, &b) in n.0.iter().enumerate() {
        if b {
            words[i / 64] |= 1u64 << (i % 64);
        }
    }
    words
}

/// Deterministic xorshift64 PRNG.
struct XorShift(u64);

impl XorShift {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

// ---------------------------------------------------------------------------
// Layer 1: ports from lib/test_bitmap.c.
// ---------------------------------------------------------------------------

/// Port of `test_zero_clear()` from lib/test_bitmap.c.
#[test]
fn kernel_test_zero_clear() {
    let mut bmap = Bitmap::filled(1024); // memset(bmap, 0xff, 128)

    assert!(expect_pbl("0-22", &bmap, 23));
    assert!(expect_pbl("0-1023", &bmap, 1024));

    /* single-word bitmaps */
    bmap.clear_region(0, 9);
    assert!(expect_pbl("9-1023", &bmap, 1024));

    bmap.zero_covering(35);
    assert!(expect_pbl("64-1023", &bmap, 1024));

    /* cross boundaries operations */
    bmap.clear_region(79, 19);
    assert!(expect_pbl("64-78,98-1023", &bmap, 1024));

    bmap.zero_covering(115);
    assert!(expect_pbl("128-1023", &bmap, 1024));

    /* Zeroing entire area */
    bmap.make_zero();
    assert!(expect_pbl("", &bmap, 1024));
}

/// Compare the first `cmp_nbits` of a bitmap against a "parsed bit list"
/// string like the kernel's `expect_eq_pbl(pbl, bmap, nbits)` (ranges
/// "0-22,40-41" style), returning pass/fail.
fn expect_pbl(pbl: &str, bm: &Bitmap, cmp_nbits: usize) -> bool {
    let mut expected = Bitmap::zeros(cmp_nbits);
    if !pbl.is_empty() {
        for part in pbl.split(',') {
            let (a, b) = match part.split_once('-') {
                Some((a, b)) => (
                    a.parse::<usize>().unwrap(),
                    b.parse::<usize>().unwrap(),
                ),
                None => {
                    let v = part.parse::<usize>().unwrap();
                    (v, v)
                }
            };
            for bit in a..=b.min(cmp_nbits - 1) {
                expected.set(bit);
            }
        }
    }
    let got: Vec<bool> = (0..cmp_nbits).map(|i| bm.get(i)).collect();
    let want: Vec<bool> = (0..cmp_nbits).map(|i| expected.get(i)).collect();
    got == want
}

/// Port of the `test_find_nth_bit()` table.
#[test]
fn kernel_test_find_nth_bit() {
    // Set bits exactly at 10,20,...,60; 80; 123 (as the kernel test builds).
    let mut bmap = Bitmap::zeros(64 * 3);
    for bit in [10, 20, 30, 40, 50, 60, 80, 123] {
        bmap.set(bit);
    }
    let words = bmap.as_words();

    let cases: [(usize, usize); 8] =
        [(10, 0), (20, 1), (30, 2), (40, 3), (50, 4), (60, 5), (80, 6), (123, 7)];
    for &(pos, nth) in &cases {
        assert_eq!(find_nth_bit(words, 64 * 3, nth), pos, "nth={nth} size=192");
        assert_eq!(find_nth_bit(words, 64 * 3 - 1, nth), pos, "nth={nth} size=191");
    }
    // Only 8 set bits => nth=8 not found in either size.
    assert_eq!(find_nth_bit(words, 64 * 3, 8), 64 * 3);
    assert_eq!(find_nth_bit(words, 64 * 3 - 1, 8), 64 * 3 - 1);

    // Round-trip: every set bit's ordinal matches its position.
    let mut ord = 0;
    for b in 0..64 * 3 {
        if bmap.get(b) {
            assert_eq!(find_nth_bit(words, 64 * 3, ord), b);
            ord += 1;
        }
    }
}

/// The kernel tests `bitmap_{read,write}` round trips via test_bitmap.c's
/// `test_bitmap_read_write()`; port the core properties here.
#[test]
fn kernel_test_read_write() {
    let mut bm = Bitmap::filled(640);
    // Zero it, then exercise aligned + crossing reads/writes.
    bm.make_zero();

    // Write patterns at unaligned offsets crossing word boundaries.
    let mut rng = XorShift(0xabcdef01);
    for _ in 0..500 {
        let nbits = 1 + rng.below(64);
        let start = rng.below(640 + 128); // allow writes past end (grows storage)
        let val = rng.next_u64();
        bm.write(val, start, nbits);
        assert_eq!(
            bm.read(start, nbits),
            val & last_word_mask(nbits),
            "roundtrip start={start} nbits={nbits}"
        );
    }

    // get/set value8 consistency.
    let mut bm = Bitmap::zeros(256);
    bm.set_value8(0xA5, 60); // crosses the first word boundary
    assert_eq!(bm.get_value8(60), 0xA5);
    assert_eq!(bm.as_words()[0] >> 60, 0x5); // low nibble of 0xA5
    assert_eq!(bm.as_words()[1] & 0xF, 0xA); // high nibble of 0xA5
}

/// `fns()` unit checks incl. the BITS_PER_LONG sentinel.
#[test]
fn fns_basics() {
    assert_eq!(fns(0b1000, 0), 3);
    assert_eq!(fns(0b1011, 0), 0);
    assert_eq!(fns(0b1011, 1), 1);
    assert_eq!(fns(0b1011, 2), 3);
    assert_eq!(fns(0b1011, 3), 64);
    assert_eq!(fns(u64::MAX, 63), 63);
    assert_eq!(fns(u64::MAX, 64), 64);
    assert_eq!(fns(0, 0), 64);
}

// ---------------------------------------------------------------------------
// Layer 2: exhaustive differential testing vs the naive oracle.
// ---------------------------------------------------------------------------

/// Pattern families for a given width: covers edges around word boundaries.
fn pattern_families(nbits: usize) -> Vec<Bitmap> {
    let mut out = Vec::new();
    fn add_pattern_from_positions(out: &mut Vec<Bitmap>, nbits: usize, pos: &[usize]) {
        let mut b = Bitmap::zeros(nbits);
        for &p in pos {
            if p < nbits {
                b.set(p);
            }
        }
        out.push(b);
    }

    out.push(Bitmap::zeros(nbits));
    out.push(Bitmap::filled(nbits));
    // Alternating bits both phases.
    let mut alt = Bitmap::zeros(nbits);
    for i in (0..nbits).step_by(2) {
        alt.set(i);
    }
    out.push(alt.clone());
    let mut alt2 = Bitmap::zeros(nbits);
    for i in (1..nbits).step_by(2) {
        alt2.set(i);
    }
    out.push(alt2);

    // Boundary positions: last bits of each word and first bits after them,
    // plus width-adjacent bits.
    let mut boundary = Vec::new();
    for w in 0..=(nbits / 64 + 1) {
        boundary.push(w * 64);
        boundary.push(w * 64 + 63);
        boundary.push(w * 64 + 1);
    }
    boundary.extend([nbits.saturating_sub(1), nbits.saturating_sub(2), 0]);
    add_pattern_from_positions(&mut out, nbits, &boundary);

    // Single-bit patterns at strategic spots.
    for &p in boundary.iter().take(24) {
        add_pattern_from_positions(&mut out, nbits, &[p]);
    }

    // Pseudo-random fills.
    let mut rng = XorShift((nbits as u64) | 1);
    for _ in 0..3 {
        let mut r = Bitmap::zeros(nbits);
        for i in 0..nbits {
            if rng.next_u64() & 1 == 1 {
                r.set(i);
            }
        }
        out.push(r);
    }
    out.retain(|b| b.len_bits() == nbits);
    out
}

#[test]
fn exhaustive_unary_queries_all_widths_0_to_70() {
    for nbits in 0..=70usize {
        for pat in pattern_families(nbits) {
            let n = naive_of(&pat);
            let words = pat.as_words();

            assert_eq!(pat.weight(), n.weight(), "weight {nbits}");
            assert_eq!(find_first_bit(words, nbits), n.find_first(), "first {nbits}");
            assert_eq!(
                find_first_zero_bit(words, nbits),
                n.find_first_zero(),
                "first0 {nbits}"
            );
            assert_eq!(find_last_bit(words, nbits), n.find_last(), "last {nbits}");
            assert_eq!(pat.is_zero(), n.0.iter().all(|&b| !b), "empty {nbits}");
            assert_eq!(pat.is_full(), n.0.iter().all(|b| *b), "full {nbits}");

            // find_next* from many starts, including starts beyond the end.
            for &start in &[0usize, 1, nbits / 2, nbits.saturating_sub(1), nbits, nbits + 7] {
                assert_eq!(
                    find_next_bit(words, nbits, start),
                    n.find_next(start),
                    "next {nbits} start {start}"
                );
                assert_eq!(
                    find_next_zero_bit(words, nbits, start),
                    n.find_next_zero(start),
                    "next0 {nbits} start {start}"
                );
            }

            // find_nth over all valid n plus out-of-range.
            let w = n.weight();
            for k in [0usize, 1, w / 2, w.saturating_sub(1), w, w + 1] {
                assert_eq!(find_nth_bit(words, nbits, k), n.find_nth(k), "nth {nbits} k {k}");
            }
        }
    }
}

fn naive_of(b: &Bitmap) -> Naive {
    Naive::from_words(b.as_words(), b.len_bits())
}

/// Wide half of the unary sweep (multi-word-plus edges beyond 70 bits).
/// Gated behind --ignored to keep default debug runs quick; run via:
///   cargo test -p kbitmap --release -- --ignored
#[test]
#[ignore]
fn exhaustive_unary_queries_widths_71_to_131() {
    for nbits in 71..=131usize {
        for pat in pattern_families(nbits) {
            let n = naive_of(&pat);
            let words = pat.as_words();

            assert_eq!(pat.weight(), n.weight(), "weight {nbits}");
            assert_eq!(find_first_bit(words, nbits), n.find_first(), "first {nbits}");
            assert_eq!(
                find_first_zero_bit(words, nbits),
                n.find_first_zero(),
                "first0 {nbits}"
            );
            assert_eq!(find_last_bit(words, nbits), n.find_last(), "last {nbits}");

            for &start in &[0usize, 1, nbits / 2, nbits.saturating_sub(1), nbits] {
                assert_eq!(find_next_bit(words, nbits, start), n.find_next(start));
                assert_eq!(find_next_zero_bit(words, nbits, start), n.find_next_zero(start));
            }
            let w = n.weight();
            for k in [0usize, 1, w.saturating_sub(1), w] {
                assert_eq!(find_nth_bit(words, nbits, k), n.find_nth(k), "nth {nbits} k {k}");
            }
        }
    }
}

#[test]
fn exhaustive_pairwise_tiny_widths() {
    // Full enumeration of ordered pairs for widths <= 7 (<=16k pairs total).
    for nbits in 0..=7usize {
        let pats: Vec<Bitmap> = (0..(1u32 << nbits))
            .map(|mask| {
                let mut b = Bitmap::zeros(nbits);
                for i in 0..nbits {
                    if mask & (1 << i) != 0 {
                        b.set(i);
                    }
                }
                b
            })
            .collect();
        for a in &pats {
            for b in &pats {
                let na = naive_of(a);
                let nb = naive_of(b);

                let mut dst = Bitmap::zeros(nbits);
                assert_eq!(dst.and(a, b), na.0.iter().zip(&nb.0).any(|(x, y)| *x && *y));
                let mut expect_and = Naive::new(nbits);
                for i in 0..nbits {
                    expect_and.0[i] = na.0[i] && nb.0[i];
                }
                assert_eq!(naive_of(&dst).0, expect_and.0, "and {nbits}");

                let mut dst = Bitmap::zeros(nbits);
                dst.or_with(a, b);
                for i in 0..nbits {
                    assert_eq!(dst.get(i), na.0[i] || nb.0[i], "or {nbits} i {i}");
                }

                let mut dst = Bitmap::zeros(nbits);
                dst.xor_with(a, b);
                for i in 0..nbits {
                    assert_eq!(dst.get(i), na.0[i] ^ nb.0[i], "xor {nbits} i {i}");
                }

                let mut dst = Bitmap::zeros(nbits);
                assert_eq!(
                    dst.andnot(a, b),
                    na.0.iter().zip(&nb.0).any(|(x, y)| *x && !*y)
                );
                for i in 0..nbits {
                    assert_eq!(dst.get(i), na.0[i] && !nb.0[i], "andnot {nbits} i {i}");
                }

                assert_eq!(a.equal(b), na.0 == nb.0, "equal {nbits}");
                assert_eq!(a.intersects(b), na.0.iter().zip(&nb.0).any(|(x, y)| *x && *y));
                assert_eq!(a.subset(b), na.0.iter().zip(&nb.0).all(|(x, y)| !x || *y));
                assert_eq!(a.or_equal(b, &pats[pats.len() - 1]), {
                    let mut tmp = Naive::new(nbits);
                    for i in 0..nbits {
                        tmp.0[i] = (na.0[i] || nb.0[i]) == na.0[i]; // placeholder replaced below
                    }
                    let _ = tmp;
                    // real check: (a|b)==c where c=all-ones is last pattern
                    (0..nbits).all(|i| na.0[i] || nb.0[i])
                });

                assert_eq!(a.weight_and(b), na.0.iter().zip(&nb.0).filter(|(x, y)| **x && **y).count());
                assert_eq!(
                    a.weight_andnot(b),
                    na.0.iter().zip(&nb.0).filter(|(x, y)| **x && !(**y)).count()
                );
            }
        }
    }
}

#[test]
#[ignore]
fn differential_binary_wide_widths() {
    for nbits in [63usize, 64, 65, 127, 128, 129, 130, 131] {
        let pats = pattern_families(nbits);
        for a in &pats {
            for b in &pats {
                let na = naive_of(a);
                let nb = naive_of(b);

                let mut dst = Bitmap::zeros(nbits);
                dst.and(a, b);
                for i in 0..nbits {
                    assert_eq!(dst.get(i), na.0[i] && nb.0[i], "and {nbits} i {i}");
                }
                dst.or_with(a, b);
                for i in 0..nbits {
                    assert_eq!(dst.get(i), na.0[i] || nb.0[i], "or {nbits} i {i}");
                }
                dst.xor_with(a, b);
                for i in 0..nbits {
                    assert_eq!(dst.get(i), na.0[i] ^ nb.0[i], "xor {nbits} i {i}");
                }
                dst.andnot(a, b);
                for i in 0..nbits {
                    assert_eq!(dst.get(i), na.0[i] && !nb.0[i], "andnot {nbits} i {i}");
                }

                let mut dst = Bitmap::zeros(nbits);
                dst.complement(a);
                for i in 0..nbits {
                    assert_eq!(dst.get(i), !na.0[i], "compl {nbits} i {i}");
                }
                assert_eq!(a.equal(b), na.0 == nb.0);
            }
        }
    }
}

#[test]
fn exhaustive_set_clear_regions_and_shifts() {
    for nbits in 0..=131usize {
        for pat in pattern_families(nbits) {
            let mut n_src = naive_of(&pat);

            // Region set/clear with clamping at the bitmap edge.
            for &start0 in &[0usize, 1, nbits / 3, nbits.saturating_sub(1)] {
                for &len in &[0usize, 1, 5, 63, 64, 65, 200] {
                    let start = start0.min(nbits);
                    let mut b = pat.clone();
                    b.set_region(start, len);
                    let mut m = n_src.0.clone();
                    let end = (start + len).min(nbits);
                    m[start..end].fill(true);
                    assert_eq!(naive_of(&b).0, m, "set_region {nbits} {start} {len}");

                    let mut b = pat.clone();
                    b.clear_region(start, len);
                    let mut m = n_src.0.clone();
                    m[start..end].fill(false);
                    assert_eq!(naive_of(&b).0, m, "clear_region {nbits} {start} {len}");
                }
            }

            // Shifts, including shift >= nbits => all zero.
            for &shift in &[0usize, 1, 7, 63, 64, 65, nbits / 2, nbits, nbits + 3] {
                let mut r = Bitmap::zeros(nbits);
                r.shift_right(&pat, shift);
                let mut m = Naive::new(nbits);
                if shift < nbits {
                    for i in 0..nbits - shift {
                        m.0[i] = n_src.0[i + shift];
                    }
                }
                assert_eq!(naive_of(&r).0, m.0, "shr {nbits} shift {shift}");

                let mut r = Bitmap::zeros(nbits);
                r.shift_left(&pat, shift);
                let mut m = Naive::new(nbits);
                if shift < nbits {
                    for i in 0..nbits - shift {
                        m.0[i + shift] = n_src.0[i];
                    }
                }
                assert_eq!(naive_of(&r).0, m.0, "shl {nbits} shift {shift}");
            }

            // cut(): every (first, cut) with first+cut <= nbits for small
            // widths; strided sweep for larger ones (keeps debug runs quick).
            let cut_stride = if nbits <= 48 { 1 } else { 7 };
            for first in (0..=nbits).step_by(cut_stride) {
                for cut in (0..=(nbits - first)).step_by(cut_stride) {
                    let mut b = pat.clone();
                    b.cut(first, cut);
                    let mut m = Naive::new(nbits);
                    for i in 0..nbits {
                        m.0[i] = if i < first {
                            n_src.0[i]
                        } else if i + cut < nbits {
                            n_src.0[i + cut]
                        } else {
                            false
                        };
                    }
                    assert_eq!(naive_of(&b).0, m.0, "cut {nbits} first {first} cut {cut}");
                }
            }

            let _ = &mut n_src;
        }
    }
}

#[test]
fn differential_find_two_and_three_regions() {
    for nbits in [1usize, 31, 63, 64, 65, 100, 127, 128, 129] {
        let pats = pattern_families(nbits);
        for a in &pats {
            for b in pats.iter().take(8) {
                let wa = a.as_words();
                let wb = b.as_words();
                let na = naive_of(a);
                let nb = naive_of(b);

                // Combined views.
                let comb_and: Vec<bool> = (0..nbits).map(|i| na.0[i] && nb.0[i]).collect();
                let comb_andnot: Vec<bool> = (0..nbits).map(|i| na.0[i] && !nb.0[i]).collect();
                let comb_or: Vec<bool> = (0..nbits).map(|i| na.0[i] || nb.0[i]).collect();

                for &start in &[0usize, 1, nbits / 2, nbits, nbits + 5] {
                    assert_eq!(
                        find_next_and_bit(wa, wb, nbits, start),
                        find_in(&comb_and, start),
                        "and {nbits} {start}"
                    );
                    assert_eq!(
                        find_next_andnot_bit(wa, wb, nbits, start),
                        find_in(&comb_andnot, start),
                        "andnot {nbits} {start}"
                    );
                    assert_eq!(
                        find_next_or_bit(wa, wb, nbits, start),
                        find_in(&comb_or, start),
                        "or {nbits} {start}"
                    );
                }

                assert_eq!(find_first_and_bit(wa, wb, nbits), find_in(&comb_and, 0));
                assert_eq!(find_first_andnot_bit(wa, wb, nbits), find_in(&comb_andnot, 0));

                // find_first_and_and_bit against a third region.
                for c in pattern_families(nbits).iter().take(4) {
                    let wc = c.as_words();
                    let nc = naive_of(c);
                    let comb3: Vec<bool> =
                        (0..nbits).map(|i| na.0[i] && nb.0[i] && nc.0[i]).collect();
                    assert_eq!(
                        find_first_and_and_bit(wa, wb, wc, nbits),
                        find_in(&comb3, 0),
                        "and3 {nbits}"
                    );
                }

                // nth variants.
                for k in [0usize, 1, 3] {
                    let nth_of = |v: &[bool], k: usize| {
                        v.iter().enumerate().filter(|(_, &x)| x).nth(k).map_or(nbits, |(i, _)| i)
                    };
                    assert_eq!(
                        find_nth_and_bit(wa, wb, nbits, k),
                        nth_of(&comb_and, k),
                        "nth_and {nbits} {k}"
                    );
                }
            }
        }
    }
}

fn find_in(comb: &[bool], start: usize) -> usize {
    if start >= comb.len() {
        return comb.len();
    }
    (start..comb.len()).find(|&i| comb[i]).unwrap_or(comb.len())
}

/// `bitmap_find_next_zero_area_off` vs brute force.
#[test]
fn differential_find_zero_area() {
    for nbits in [1usize, 63, 64, 65, 100, 128, 200] {
        for pat in pattern_families(nbits) {
            let words_owned = pat.as_words().to_vec();
            let n = naive_of(&pat);
            for &align_mask in &[0usize, 7, 63] {
                for &nr in &[1usize, 3, 8, 17] {
                    for &start in &[0usize, 5, nbits / 2] {
                        let got =
                            find_next_zero_area_off(&words_owned, nbits, start, nr, align_mask, 0);
                        let want = brute_zero_area(&n.0, start, nr, align_mask, 0);
                        assert_eq!(got >= nbits, want.is_none(), "{nbits}/{nr}/{align_mask}/{start}");
                        if let Some(want) = want {
                            assert_eq!(got, want, "{nbits} nr{nr} am{align_mask} s{start}");
                        }
                    }
                }
            }
        }
    }
}

fn brute_zero_area(
    bits: &[bool],
    start: usize,
    nr: usize,
    align_mask: usize,
    align_offset: usize,
) -> Option<usize> {
    let mut s = start;
    loop {
        s = (s..bits.len()).find(|&i| !bits[i])?;
        s = ((s + align_offset) | align_mask) - align_offset;
        if s >= bits.len() {
            return None;
        }
        let end = s + nr;
        if end > bits.len() {
            return None;
        }
        if !bits[s..end].iter().any(|&b| b) {
            return Some(s);
        }
        // jump past the blocking set bit, like the kernel does
        s = bits[s..end].iter().rposition(|&b| b).map_or(end, |p| s + p);
    }
}

// ---------------------------------------------------------------------------
// Layer 3: stateful randomized model check.
// ---------------------------------------------------------------------------

#[test]
fn stateful_model_check() {
    let mut rng = XorShift(0x5eed_0001);
    for case in 0..40u32 {
        let nbits = 1 + rng.below(300);
        let mut real = Bitmap::zeros(nbits);
        let mut model = Naive::new(nbits);

        for step in 0..400 {
            match rng.below(12) {
                0 => {
                    let i = rng.below(nbits);
                    real.set(i);
                    model.0[i] = true;
                }
                1 => {
                    let i = rng.below(nbits);
                    real.clear(i);
                    model.0[i] = false;
                }
                2 => {
                    let i = rng.below(nbits);
                    real.assign(i, step % 2 == 0);
                    model.0[i] = step % 2 == 0;
                }
                3 => {
                    let s = rng.below(nbits);
                    let l = rng.below(nbits + 1);
                    real.set_region(s, l);
                    for i in s..(s + l).min(nbits) {
                        model.0[i] = true;
                    }
                }
                4 => {
                    let s = rng.below(nbits);
                    let l = rng.below(nbits + 1);
                    real.clear_region(s, l);
                    for i in s..(s + l).min(nbits) {
                        model.0[i] = false;
                    }
                }
                5 => {
                    real.fill();
                    model.0.iter_mut().for_each(|m| *m = true);
                }
                6 => {
                    real.make_zero();
                    model.0.iter_mut().for_each(|m| *m = false);
                }
                7 => {
                    // complement in place via temp
                    let mut tmp = Bitmap::zeros(nbits);
                    tmp.complement(&real);
                    real.copy_from(&tmp);
                    model.0.iter_mut().for_each(|m| *m = !*m);
                }
                8 => {
                    let sh = rng.below(nbits + 10);
                    let mut tmp = Bitmap::zeros(nbits);
                    tmp.shift_left(&real, sh);
                    real.copy_from(&tmp);
                    let old = model.0.clone();
                    for i in 0..nbits {
                        model.0[i] = sh < nbits && i >= sh && old[i - sh];
                    }
                }
                9 => {
                    let sh = rng.below(nbits + 10);
                    let mut tmp = Bitmap::zeros(nbits);
                    tmp.shift_right(&real, sh);
                    real.copy_from(&tmp);
                    let old = model.0.clone();
                    for i in 0..nbits {
                        model.0[i] = sh < nbits && i + sh < nbits && old[i + sh];
                    }
                }
                10 | 11 => {
                    let i = rng.below(nbits);
                    assert_eq!(real.get(i), model.0[i], "case {case} step {step}");
                }
                _ => unreachable!(),
            }
            // Invariant: trailing bits stay zero.
            if let Some(last) = real.as_words().last() {
                if nbits % 64 != 0 {
                    assert_eq!(last & !last_word_mask(nbits), 0, "tail invariant");
                }
            }
        }
        assert_eq!(real.as_words(), &words_of(&model)[..], "final case {case}");
    }
}
