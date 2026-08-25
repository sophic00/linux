//! Tests for the `lib/sort.c` / `lib/bsearch.c` rewrite, including the
//! randomized differential checks from `lib/tests/test_sort.c`.

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;

use alloc::{vec, vec::Vec};

use krand::{Krand, Rng};

use super::*;

#[test]
fn empty_and_single() {
    let mut v: Vec<u32> = vec![];
    heapsort(&mut v);
    assert!(v.is_empty());

    let mut v = vec![42u32];
    heapsort(&mut v);
    assert_eq!(v, [42]);
}

#[test]
fn two_elements() {
    assert_eq!(
        {
            let mut v = vec![2u32, 1];
            heapsort(&mut v);
            v
        },
        [1, 2]
    );
    assert_eq!(
        {
            let mut v = vec![1u32, 2];
            heapsort(&mut v);
            v
        },
        [1, 2]
    );
    assert_eq!(
        {
            let mut v = vec![7u32, 7];
            heapsort(&mut v);
            v
        },
        [7, 7]
    );
}

#[test]
fn three_elements() {
    // Exercises the n == 3*size endgame path.
    let cases: &[Vec<u32>] = &[
        vec![3, 2, 1],
        vec![1, 2, 3],
        vec![2, 1, 3],
        vec![2, 3, 1],
        vec![5, 5, 5],
    ];
    for c in cases {
        let mut v = c.clone();
        heapsort(&mut v);
        let mut expect = c.clone();
        expect.sort_unstable();
        assert_eq!(v, expect, "case {c:?}");
    }
}

/// The kernel's test_sort.c sorts an array of (value, ptr) pairs with a
/// custom comparator and verifies the pointers stay attached to their values.
#[test]
fn payload_stays_attached() {
    #[derive(Debug)]
    struct Elem {
        key: u32,
        tag: usize,
    }
    const N: usize = 4096;
    let mut rng = Krand::seed_from_u64(0x1234_5678);

    let mut v: Vec<Elem> = (0..N)
        .map(|i| Elem {
            key: rng.below(100) as u32,
            tag: i,
        })
        .collect();

    let mut expect: Vec<(u32, usize)> = v.iter().map(|e| (e.key, e.tag)).collect();

    heapsort_by(&mut v, |a, b| a.key.cmp(&b.key));

    // Keys must be non-decreasing...
    for w in v.windows(2) {
        assert!(w[0].key <= w[1].key);
    }
    // ...tags must still be a permutation of 0..N (nothing lost/duplicated)...
    let mut tags: Vec<usize> = v.iter().map(|e| e.tag).collect();
    tags.sort_unstable();
    assert_eq!(tags, Vec::from_iter(0..N));
    // ...and the sorted key multiset must match a std-sorted copy.
    expect.sort_by_key(|&(k, _)| k);
    let want: Vec<u32> = expect.into_iter().map(|(k, _)| k).collect();
    let got: Vec<u32> = v.iter().map(|e| e.key).collect();
    ktest_util::assert_same_elements(&got, &want);
}

/// Differential fuzz against the standard library's sort.
#[test]
fn differential_vs_std() {
    for &n in &[0usize, 1, 2, 3, 4, 5, 10, 33, 64, 100, 1000, 5000] {
        for seed in 0..16u64 {
            // Per-(n, seed) independent stream; same iteration counts and
            // value ranges as before.
            let mut rng = Krand::seed_from_u64(seed ^ 0xdead_beef);

            // Random values.
            let mut v: Vec<u32> = (0..n).map(|_| rng.below(50) as u32).collect();
            let expect = v.clone();
            heapsort(&mut v);
            let mut e = expect;
            e.sort_unstable();
            assert_eq!(v, e, "random n={n} seed={seed}");

            // Already sorted (best case for sift-down).
            let mut v: Vec<u32> = (0..n as u32).collect();
            heapsort(&mut v);
            assert_eq!(v, (0..n as u32).collect::<Vec<_>>(), "sorted n={n}");

            // Reverse sorted.
            let mut v: Vec<u32> = (0..n as u32).rev().collect();
            heapsort(&mut v);
            assert_eq!(v, (0..n as u32).collect::<Vec<_>>(), "reversed n={n}");

            // All equal.
            let mut v: Vec<u32> = vec![7; n];
            heapsort(&mut v);
            assert_eq!(v, vec![7; n], "equal n={n}");

            // Descending comparator exercises the other comparison branch.
            let mut v: Vec<u32> = (0..n as u32).map(|_| rng.below(97) as u32).collect();
            let mut e = v.clone();
            heapsort_by(&mut v, |a, b| b.cmp(a));
            e.sort_unstable_by(|a, b| b.cmp(a));
            assert_eq!(v, e, "desc n={n} seed={seed}");
        }
    }
}

/// Exhaustive check over all permutations of small arrays.
#[test]
fn exhaustive_small_permutations() {
    let data = [3u32, 1, 2, 0];
    check_perms(&mut data.to_vec(), &[], &data);

    fn check_perms(rest: &mut [u32], prefix: &[u32], full: &[u32]) {
        if rest.is_empty() {
            let mut v = prefix.to_vec();
            heapsort(&mut v);
            let mut e = full.to_vec();
            e.sort_unstable();
            assert_eq!(v, e, "perm {prefix:?}");
            return;
        }
        for i in 0..rest.len() {
            rest.swap(0, i);
            let mut p = prefix.to_vec();
            p.push(rest[0]);
            check_perms(&mut rest[1..], &p, full);
            rest.swap(0, i);
        }
    }
}

#[test]
fn bsearch_finds_elements() {
    let v: Vec<u32> = (0..1000).map(|x| x * 3).collect(); // sorted multiples of 3

    for k in 0..3000u32 {
        let idx = bsearch_by(&k, &v, |key, item| key.cmp(item));
        if k % 3 == 0 {
            assert_eq!(idx, Some((k / 3) as usize), "key {k}");
        } else {
            assert_eq!(idx, None, "key {k}");
        }
    }
}

#[test]
fn bsearch_empty() {
    let v: Vec<u32> = vec![];
    assert_eq!(bsearch_by(&1, &v, |k, i| k.cmp(i)), None);
}

#[test]
fn sort_r_matches_c_reference_semantics() {
    // Sort by one field while verifying total ordering afterwards.
    let mut v: Vec<(u32, char)> = vec![
        (5, 'a'),
        (1, 'b'),
        (4, 'c'),
        (1, 'd'),
        (3, 'e'),
        (9, 'f'),
        (2, 'g'),
        (6, 'h'),
    ];
    heapsort_by(&mut v, |a, b| a.0.cmp(&b.0));
    let keys: Vec<u32> = v.iter().map(|x| x.0).collect();
    assert_eq!(keys, [1, 1, 2, 3, 4, 5, 6, 9]);

    // Verify the algorithm is comparator-agnostic.
    let mut v: Vec<i32> = (-8..8).collect();
    let mut e = v.clone();
    heapsort_by(&mut v, |a, b| b.cmp(a));
    e.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(v, e);
}
