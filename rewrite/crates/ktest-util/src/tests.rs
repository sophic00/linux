//! Tests for the `ktest-util` helpers themselves.

extern crate alloc;

use alloc::{vec, vec::Vec};

use crate::{assert_same_elements, assert_sorted, run_fuzz};
use krand::Rng;

#[test]
fn same_elements_accepts_permutations_and_duplicates() {
    assert_same_elements(&[3, 1, 2], &[2, 3, 1]);
    assert_same_elements(&[1, 1, 2], &[2, 1, 1]);
    assert_same_elements::<u8>(&[], &[]);
    let empty: Vec<u32> = vec![];
    assert_same_elements(&empty, &[]);
}

#[test]
#[should_panic(expected = "missing 9")]
fn same_elements_reports_missing() {
    assert_same_elements(&[1, 2, 3], &[1, 2, 9]);
}

#[test]
#[should_panic(expected = "unexpected 7")]
fn same_elements_reports_unexpected() {
    assert_same_elements(&[1, 7], &[1, 2]);
}

#[test]
#[should_panic(expected = "got 2x want 1x")]
fn same_elements_reports_multiplicity_mismatch() {
    assert_same_elements(&[5, 5], &[5]);
}

#[test]
fn sorted_accepts_non_decreasing_including_equal_runs() {
    assert_sorted::<u8>(&[]);
    assert_sorted(&[1]);
    assert_sorted(&[1, 2, 2, 3]);
}

#[test]
#[should_panic(expected = "not sorted")]
fn sorted_rejects_descending_pair() {
    assert_sorted(&[1, 3, 2]);
}

#[test]
fn run_fuzz_is_replayable_iteration_by_iteration() {
    // Record what each iteration produced...
    let mut seen: Vec<(u64, Vec<u64>)> = Vec::new();
    run_fuzz(0xC0FFEE, 8, |rng, i| {
        let draws: Vec<u64> = (0..4).map(|_| rng.next_u64()).collect();
        seen.push((i, draws));
    });

    // ...then replay a *single* late iteration in isolation: it must match.
    let mut solo: Option<Vec<u64>> = None;
    run_fuzz(0xC0FFEE, 8, |rng, i| {
        if i == 6 {
            solo = Some((0..4).map(|_| rng.next_u64()).collect());
        } else {
            for _ in 0..4 {
                rng.next_u64();
            }
        }
    });
    assert_eq!(seen[6].0, 6);
    assert_eq!(solo.as_ref().unwrap(), &seen[6].1);
}

#[test]
fn run_fuzz_iterations_are_independent_streams() {
    // Two iterations of the same base seed must not share leading draws
    // (derive_seed spreads them), otherwise per-iteration seeding is fake.
    let mut firsts: Vec<u64> = Vec::new();
    run_fuzz(42, 64, |rng, _| {
        firsts.push(rng.next_u64());
    });
    let unique = {
        let mut c = firsts.clone();
        c.sort_unstable();
        c.dedup();
        c.len()
    };
    assert_eq!(unique, firsts.len(), "iterations shared RNG output");
}
