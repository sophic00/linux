//! Tests for the `lib/list_sort.c` rewrite: ported ideas from
//! `lib/tests/test_list_sort.c` (random lists verified sorted, comparison
//! budget check) plus exhaustive small-size permutations, explicit
//! stability checks, and multiset equality against `std`'s stable sort.

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;

use alloc::{vec, vec::Vec};

use krand::{Krand, Rng};

use super::*;

/// Builds a list of (key, tag) pairs; tags record the input order.
fn make_list(keys: &[u32]) -> List<(u32, usize)> {
    keys.iter()
        .enumerate()
        .map(|(i, &k)| (k, i))
        .collect::<List<_>>()
}

fn keys_of(l: &List<(u32, usize)>) -> Vec<u32> {
    l.iter().map(|&(k, _)| k).collect()
}

fn tags_of(l: &List<(u32, usize)>) -> Vec<usize> {
    l.iter().map(|&(_, t)| t).collect()
}

#[test]
fn empty_and_single() {
    let mut l: List<u32> = List::new();
    assert!(l.is_empty());
    l.list_sort(|a, b| a.cmp(b));
    assert!(l.is_empty());

    let mut l = List::from_iter([42u32]);
    l.list_sort(|a, b| a.cmp(b));
    assert_eq!(l.into_iter_list().next(), Some(42));
}

/// Verifies sortedness, stability, and length preservation.
fn check_sorted_stable(l: &List<(u32, usize)>, n_expected: usize) {
    assert_eq!(l.len(), n_expected);
    let ks = keys_of(l);
    for w in ks.windows(2) {
        assert!(w[0] <= w[1], "not sorted: {ks:?}");
    }
    // Stability: equal keys keep their original relative order.
    let ts = tags_of(l);
    for i in 0..ts.len() {
        for j in 0..i {
            if ks[j] == ks[i] {
                assert!(ts[j] < ts[i], "unstable at {j},{i}: {ts:?}");
            }
        }
    }
}

#[test]
fn exhaustive_small_sizes() {
    // All sequences over a 3-symbol alphabet up to length 5 (incl. many
    // duplicate patterns): exercises every pending-run state transition.
    const ALPHA: u32 = 3;
    let total = (ALPHA as usize..).take(6).product::<usize>(); // 3^6 bound
    let _ = total;
    let mut seq = Vec::new();
    for len in 0..=5usize {
        let cases = ALPHA.pow(len as u32) as usize;
        for code in 0..cases {
            seq.clear();
            let mut c = code;
            for _ in 0..len {
                seq.push((c % ALPHA as usize) as u32);
                c /= ALPHA as usize;
            }
            let mut l = make_list(&seq);
            l.list_sort(|a, b| a.0.cmp(&b.0));
            check_sorted_stable(&l, len);
            // Multiset equality vs std.
            let mut e = seq.clone();
            e.sort_unstable();
            assert_eq!(keys_of(&l), e, "code {code} len {len}");
        }
    }
}

#[test]
fn already_reverse_and_equal() {
    let sorted: Vec<u32> = (0..100).collect();

    let mut l = make_list(&sorted);
    l.list_sort(|a, b| a.0.cmp(&b.0));
    check_sorted_stable(&l, 100);
    assert_eq!(keys_of(&l), sorted);

    let rev: Vec<u32> = (0..100).rev().collect();
    let mut l = make_list(&rev);
    l.list_sort(|a, b| a.0.cmp(&b.0));
    check_sorted_stable(&l, 100);
    assert_eq!(keys_of(&l), sorted);

    let all_eq: Vec<u32> = vec![7; 257];
    let mut l = make_list(&all_eq);
    l.list_sort(|a, b| a.0.cmp(&b.0));
    check_sorted_stable(&l, 257);
    assert_eq!(keys_of(&l), all_eq);
}

/// Ported idea from lib/tests/test_list_sort.c: random lists must come out
/// sorted with every element preserved.
#[test]
fn random_fuzz_matches_std() {
    let mut rng = Krand::seed_from_u64(0x1234_5678_dead_beef);

    for &n in &[
        2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 255, 256, 257, 511, 512,
        513, 1000, 4096,
    ] {
        let keys: Vec<u32> = (0..n).map(|_| (rng.below(50)) as u32).collect();
        let mut l = make_list(&keys);
        l.list_sort(|a, b| a.0.cmp(&b.0));
        check_sorted_stable(&l, n);

        let mut expect: Vec<(u32, usize)> = keys.iter().copied().zip(0..n).collect();
        expect.sort_by_key(|&(k, _)| k); // std stable sort
        let want_tags: Vec<usize> = expect.iter().map(|&(_, t)| t).collect();
        assert_eq!(tags_of(&l), want_tags, "stability mismatch n={n}");
    }
}

/// The kernel test also asserts list_sort performs fewer than
/// n * ceil(log2(n)) + n comparisons ("efficiency" check).
#[test]
fn comparison_budget() {
    use core::cell::Cell;
    let counter = Cell::new(0usize);
    let mut cmp = |a: &(u32, usize), b: &(u32, usize)| {
        counter.set(counter.get() + 1);
        a.0.cmp(&b.0)
    };

    let mut rng = Krand::seed_from_u64(0xfeed_c0de);
    for &n in &[16usize, 64, 256, 1024, 4096] {
        let keys: Vec<u32> = (0..n).map(|_| rng.below(u32::MAX as u64) as u32).collect();
        let mut l = make_list(&keys);
        counter.set(0);
        l.list_sort(&mut cmp);
        let budget = n * (usize::BITS - (n as u64).leading_zeros()) as usize + n;
        assert!(
            counter.get() < budget,
            "n={n}: {} comparisons >= budget {budget}",
            counter.get()
        );
    }
}

#[test]
fn descending_comparator() {
    let keys: Vec<u32> = vec![3, 1, 4, 1, 5, 9, 2, 6];
    let mut l = make_list(&keys);
    l.list_sort(|a, b| b.0.cmp(&a.0)); // descending: cmp(a,b)>0 means a first
    let got = keys_of(&l);
    let mut e = keys.clone();
    e.sort_by(|a, b| b.cmp(a));
    assert_eq!(got, e);
}

#[test]
fn push_pop_iter_roundtrip() {
    let mut l: List<u32> = List::new();
    l.push_front(1);
    l.push_front(2);
    l.push_front(3);
    assert_eq!(l.len(), 3);
    assert_eq!(l.iter().copied().collect::<Vec<_>>(), vec![3, 2, 1]);
    assert_eq!(l.pop_front(), Some(3));
    assert_eq!(l.pop_front(), Some(2));
    assert_eq!(l.len(), 1);
}
