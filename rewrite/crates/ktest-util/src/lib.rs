// SPDX-License-Identifier: GPL-2.0
//! Shared test helpers for the Linux kernel Rust rewrite workspace.
//!
//! No direct C counterpart: this crate is the workspace's equivalent of the
//! common plumbing inside kernel KUnit suites (expectation helpers, random
//! list builders). It exists so per-crate `tests.rs` files assert with one
//! voice instead of re-implementing multiset diffs and fuzz loops.

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::fmt::Debug;
use krand::{derive_seed, Krand};

/// Asserts that two slices contain exactly the same elements, ignoring
/// order and multiplicity-free comparison of *counts* — i.e. true multiset
/// equality.
///
/// On failure the panic message reports which values are missing from `got`
/// and which are unexpected, with counts, instead of dumping two huge
/// vectors like a raw `assert_eq!` on sorted copies would.
///
/// This is the standard oracle for "the sort/search/shuffle preserved all
/// data" checks across the workspace.
pub fn assert_same_elements<T>(got: &[T], want: &[T])
where
    T: Ord + Clone + Debug,
{
    fn counts<T: Ord + Clone>(s: &[T]) -> Vec<(T, usize)> {
        let mut v = s.to_vec();
        v.sort();
        let mut out: Vec<(T, usize)> = Vec::new();
        for x in v {
            match out.last_mut() {
                Some((last, n)) if *last == x => *n += 1,
                _ => out.push((x, 1)),
            }
        }
        out
    }

    let cg = counts(got);
    let cw = counts(want);
    if cg == cw {
        return;
    }

    // Build missing / extra reports by walking both count lists.
    let mut report = String::from("element multisets differ");
    let mut i = 0usize;
    let mut j = 0usize;
    while i < cg.len() && j < cw.len() {
        use core::cmp::Ordering::*;
        match cg[i].0.cmp(&cw[j].0) {
            Equal => {
                if cg[i].1 != cw[j].1 {
                    report.push_str(&format!(
                        "; {:?}: got {}x want {}x",
                        cg[i].0, cg[i].1, cw[j].1
                    ));
                }
                i += 1;
                j += 1;
            }
            Less => {
                report.push_str(&format!("; unexpected {:?}", cg[i].0));
                i += 1;
            }
            Greater => {
                report.push_str(&format!("; missing {:?}", cw[j].0));
                j += 1;
            }
        }
    }
    for c in &cg[i..] {
        report.push_str(&format!("; unexpected {:?}", c.0));
    }
    for c in &cw[j..] {
        report.push_str(&format!("; missing {:?}", c.0));
    }
    panic!("{report} (got {} elements, want {})", got.len(), want.len());
}

/// Asserts `xs` is non-decreasing under `Ord` — the "is it actually sorted"
/// half of every sort test.
pub fn assert_sorted<T: PartialOrd + Debug>(xs: &[T]) {
    for w in xs.windows(2) {
        assert!(
            w[0] <= w[1],
            "not sorted at pair ({:?}, {:?}) in {xs:?}",
            w[0],
            w[1]
        );
    }
}

/// Fuzz driver: runs `f` for `iterations`, handing each iteration its own
/// [`Krand`] seeded deterministically from `(base_seed, i)` via
/// [`krand::derive_seed`].
///
/// Per-iteration seeding means a failure prints everything needed to replay
/// exactly one iteration, independent of how many iterations ran before it.
/// The callback receives the iteration index as second argument.
///
/// Allocation-light and timing-free: no clocks, no entropy, no I/O.
pub fn run_fuzz<F>(base_seed: u64, iterations: u64, mut f: F)
where
    F: FnMut(&mut Krand, u64),
{
    for i in 0..iterations {
        let mut rng = Krand::seed_from_u64(derive_seed(base_seed, i));
        f(&mut rng, i);
    }
}

#[cfg(test)]
mod tests;
