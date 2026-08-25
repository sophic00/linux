//! Tests for the `lib/min_heap.c` rewrite.
//!
//! Layer 1: faithful port of the kernel's own suite
//! `lib/tests/min_heap_kunit.c` (all four parametrized cases, min and max
//! variants, including the exact constant arrays — note the KUnit file
//! deliberately uses `0x8000000`/`0x7FFFFFF` in the heapify/del tests but
//! `0x80000000`/`0x7FFFFFFF` in the push/pop_push tests; that distinction is
//! preserved).
//!
//! Layer 2: property/fuzz testing with three independent oracles:
//! - a `BTreeMap` multiset oracle (contents + pop-order equivalence),
//! - `std::collections::BinaryHeap` (push/pop/pop_push differential),
//! - an O(n^2) linear-selection reference for drained order, which shares no
//!   code path with any heap algorithm.
//!
//! A full heap-invariant checker runs after every operation.

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Reverse;
extern crate std;

use std::collections::{BTreeMap, BinaryHeap};

use super::*;

// -----------------------------------------------------------------------
// Shared helpers
// -----------------------------------------------------------------------

/// C `less_than`.
fn less_than(a: &i32, b: &i32) -> bool {
    a < b
}

/// C `greater_than`.
fn greater_than(a: &i32, b: &i32) -> bool {
    a > b
}

/// Deterministic stand-in for `get_random_u32()` (xorshift64, high bits out).
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
}

/// Heap-invariant checker: for every node, no child may be "less" than its
/// parent. Runs after every mutation in the fuzz layer.
fn check_invariant<T, F>(v: &[T], mut less: F)
where
    F: FnMut(&T, &T) -> bool,
{
    for i in 1..v.len() {
        assert!(
            !less(&v[i], &v[(i - 1) / 2]),
            "heap invariant violated at index {i}: child not >= parent"
        );
    }
}

/// C `pop_verify_heap`: pop everything; successive minimums must be
/// monotonically non-decreasing (`min`) or non-increasing (`max`).
fn pop_verify_heap(min_heap: bool, heap: &mut MinHeap<i32>) {
    let mut funcs: fn(&i32, &i32) -> bool = if min_heap { less_than } else { greater_than };
    let Some(mut last) = heap.pop(&mut funcs) else {
        return;
    };
    while let Some(cur) = heap.pop(&mut funcs) {
        if min_heap {
            assert!(last <= cur, "min-heap popped {last} then {cur}");
        } else {
            assert!(last >= cur, "max-heap popped {last} then {cur}");
        }
        last = cur;
    }
}

// -----------------------------------------------------------------------
// Layer 1 — port of lib/tests/min_heap_kunit.c
// -----------------------------------------------------------------------

/// Values exactly as in KUnit `test_heapify_all` / `test_heap_del`
/// (note: 0x8000000 / 0x7FFFFFF, *not* the INT_MIN/MAX pair).
const KUNIT_VALUES_SMALL_EXTREMES: [i32; 13] = [
    3, 1, 2, 4, 0x8000000, 0x7FFFFFF, 0, -3, -1, -2, -4, 0x8000000, 0x7FFFFFF,
];

/// Values exactly as in KUnit `test_heap_push` / `test_heap_pop_push`
/// (note: i32::MIN (C 0x80000000) and 0x7FFFFFFF = i32::MAX here).
const KUNIT_VALUES_INT_EXTREMES: [i32; 13] = [
    3,
    1,
    2,
    4,
    i32::MIN,
    0x7FFFFFFF,
    0,
    -3,
    -1,
    -2,
    -4,
    i32::MIN,
    0x7FFFFFFF,
];

#[test]
fn kunit_test_heapify_all() {
    for &min_heap in &[true, false] {
        // Test with known set of values.
        let mut funcs: fn(&i32, &i32) -> bool = if min_heap { less_than } else { greater_than };
        let mut heap = MinHeap::from_array(Vec::from(KUNIT_VALUES_SMALL_EXTREMES));
        heapify_all(heap.as_mut_slice(), &mut funcs);
        pop_verify_heap(min_heap, &mut heap);

        // Test with randomly generated values.
        let mut rng = Rng::new(0x1234_5678_9abc_def0);
        let mut values = Vec::from(KUNIT_VALUES_SMALL_EXTREMES);
        for v in values.iter_mut() {
            *v = rng.next_u32() as i32;
        }
        let mut heap = MinHeap::from_array(values);
        heapify_all(heap.as_mut_slice(), &mut funcs);
        pop_verify_heap(min_heap, &mut heap);
    }
}

#[test]
fn kunit_test_heap_push() {
    for &min_heap in &[true, false] {
        let mut funcs: fn(&i32, &i32) -> bool = if min_heap { less_than } else { greater_than };

        // Test with known set of values copied from data.
        let mut heap = MinHeap::with_storage(vec![0; KUNIT_VALUES_INT_EXTREMES.len()]);
        for &d in KUNIT_VALUES_INT_EXTREMES.iter() {
            assert!(heap.push(d, &mut funcs));
        }
        pop_verify_heap(min_heap, &mut heap);

        // Test with randomly generated values (fill to capacity).
        let mut rng = Rng::new(0x00dd_ba11_cafe_f00d);
        let mut heap = MinHeap::with_storage(vec![0; KUNIT_VALUES_INT_EXTREMES.len()]);
        while !heap.is_full() {
            let temp = rng.next_u32() as i32;
            assert!(heap.push(temp, &mut funcs));
        }
        pop_verify_heap(min_heap, &mut heap);
    }
}

#[test]
fn kunit_test_heap_pop_push() {
    for &min_heap in &[true, false] {
        let mut funcs: fn(&i32, &i32) -> bool = if min_heap { less_than } else { greater_than };
        let n = KUNIT_VALUES_INT_EXTREMES.len();

        // Fill values with data to pop and replace.
        let temp = if min_heap { i32::MIN } else { i32::MAX };

        // Test with known set of values copied from data.
        let mut heap = MinHeap::with_storage(vec![0; n]);
        for _ in 0..n {
            assert!(heap.push(temp, &mut funcs));
        }
        for &d in KUNIT_VALUES_INT_EXTREMES.iter() {
            heap.pop_push(d, &mut funcs);
        }
        pop_verify_heap(min_heap, &mut heap);

        // Test with randomly generated values.
        let mut rng = Rng::new(0xfeed_face_dead_beef);
        let mut heap = MinHeap::with_storage(vec![0; n]);
        for _ in 0..n {
            assert!(heap.push(temp, &mut funcs));
        }
        for _ in 0..n {
            let t = rng.next_u32() as i32;
            heap.pop_push(t, &mut funcs);
        }
        pop_verify_heap(min_heap, &mut heap);
    }
}

#[test]
fn kunit_test_heap_del() {
    for &min_heap in &[true, false] {
        let mut funcs: fn(&i32, &i32) -> bool = if min_heap { less_than } else { greater_than };
        let mut rng = Rng::new(0xabcd_ef01_2345_6789);

        // Test with known set of values.
        let mut heap = MinHeap::from_array(Vec::from(KUNIT_VALUES_SMALL_EXTREMES));
        heapify_all(heap.as_mut_slice(), &mut funcs);
        for _ in 0..KUNIT_VALUES_SMALL_EXTREMES.len() / 2 {
            heap.del(rng.next_u32() as usize % heap.nr(), &mut funcs);
        }
        pop_verify_heap(min_heap, &mut heap);

        // Test with randomly generated values.
        let mut rng = Rng::new(0x9876_5432_10fe_dcba);
        let mut values = Vec::from(KUNIT_VALUES_SMALL_EXTREMES);
        for v in values.iter_mut() {
            *v = rng.next_u32() as i32;
        }
        let mut heap = MinHeap::from_array(values);
        heapify_all(heap.as_mut_slice(), &mut funcs);
        for _ in 0..KUNIT_VALUES_SMALL_EXTREMES.len() / 2 {
            heap.del(rng.next_u32() as usize % heap.nr(), &mut funcs);
        }
        pop_verify_heap(min_heap, &mut heap);
    }
}

// -----------------------------------------------------------------------
// Layer 2 — invariant + oracle fuzzing
// -----------------------------------------------------------------------

/// Oracle state: our heap, a multiset of the same contents, and the op log
/// length. After every operation: heap invariant holds, and the heap's
/// contents equal the multiset's contents exactly.
struct Oracle {
    heap: MinHeap<i32>,
    multiset: BTreeMap<i32, usize>,
    total: usize,
    /// true = min-heap ordering under `less`; false = max-heap.
    min_heap: bool,
}

impl Oracle {
    fn new(cap: usize, min_heap: bool) -> Self {
        Oracle {
            heap: MinHeap::with_storage(vec![0; cap]),
            multiset: BTreeMap::new(),
            total: 0,
            min_heap,
        }
    }

    /// The element the heap must produce next: multiset min for a min-heap,
    /// multiset max for a max-heap.
    fn expect_next(&self) -> i32 {
        if self.min_heap {
            *self.multiset.keys().next().expect("oracle empty")
        } else {
            *self.multiset.keys().next_back().expect("oracle empty")
        }
    }

    fn remove_one(&mut self, v: i32) {
        let e = self.multiset.get_mut(&v).unwrap();
        *e -= 1;
        if *e == 0 {
            self.multiset.remove(&v);
        }
        self.total -= 1;
    }

    /// O(n^2)-free contents comparison: expand multiset into sorted Vec and
    /// compare against the sorted heap slice.
    fn assert_contents_match(&self) {
        let mut want: Vec<i32> = self
            .multiset
            .iter()
            .flat_map(|(&k, &c)| core::iter::repeat_n(k, c))
            .collect();
        want.sort_unstable();
        let mut got = Vec::from(self.heap.as_slice());
        got.sort_unstable();
        assert_eq!(got, want, "heap contents diverged from multiset oracle");
    }

    fn sync(&mut self, less: &mut fn(&i32, &i32) -> bool) {
        check_invariant(self.heap.as_slice(), *less);
        self.assert_contents_match();
    }

    fn push(&mut self, v: i32, less: &mut fn(&i32, &i32) -> bool) {
        if self.heap.nr() >= self.heap.size() {
            // Full heap: C WARN_ONCE path; push must fail and change nothing.
            assert!(!self.heap.push(v, less), "push onto full heap must fail");
            return;
        }
        assert!(self.heap.push(v, less));
        *self.multiset.entry(v).or_insert(0) += 1;
        self.total += 1;
        self.sync(less);
    }

    fn pop(&mut self, less: &mut fn(&i32, &i32) -> bool) {
        let expect = self.expect_next();
        let got = self.heap.pop(less);
        assert_eq!(got, Some(expect), "pop did not yield the ordered extremum");
        self.remove_one(expect);
        self.sync(less);
    }

    fn pop_push(&mut self, v: i32, less: &mut fn(&i32, &i32) -> bool) {
        let expect = self.expect_next();
        let got = self.heap.pop_push(v, less);
        assert_eq!(
            got,
            Some(expect),
            "pop_push did not yield the ordered extremum"
        );
        // Count-neutral: remove the old extremum, insert v.
        let e = self.multiset.get_mut(&expect).unwrap();
        *e -= 1;
        if *e == 0 {
            self.multiset.remove(&expect);
        }
        *self.multiset.entry(v).or_insert(0) += 1;
        self.sync(less);
    }

    fn del(&mut self, idx: usize, less: &mut fn(&i32, &i32) -> bool) {
        let victim = self.heap.as_slice()[idx];
        let got = self.heap.del(idx, less);
        assert_eq!(got, Some(victim), "del({idx}) returned wrong element");
        self.remove_one(victim);
        self.sync(less);
    }
}

/// Randomized operation sequences against the multiset oracle, both orders.
#[test]
fn fuzz_ops_vs_multiset_oracle() {
    let mut rng = Rng::new(0x5eed_0001_600d_c0de);
    for round in 0..64u32 {
        for &min_heap in &[true, false] {
            let mut less: fn(&i32, &i32) -> bool = if min_heap { less_than } else { greater_than };
            let mut o = Oracle::new(96, min_heap);
            for step in 0..256 {
                match rng.next_u32() % 8 {
                    0..=3 => {
                        if o.heap.nr() < o.heap.size() {
                            o.push(rng.next_u32() as i32 % 1000, &mut less);
                        }
                    }
                    4 => {
                        if o.total > 0 {
                            o.pop(&mut less);
                        }
                    }
                    5 => {
                        if o.total > 0 {
                            o.pop_push(rng.next_u32() as i32 % 1000, &mut less);
                        }
                    }
                    _ => {
                        if o.total > 0 {
                            let idx = rng.next_u32() as usize % o.heap.nr();
                            o.del(idx, &mut less);
                        }
                    }
                }
                let _ = (round, step);
            }
            // Drain: pop order must be ascending multiset extraction.
            while o.total > 0 {
                o.pop(&mut less);
            }
            assert!(o.heap.is_empty());
        }
    }
}

/// Differential against `std::collections::BinaryHeap`: identical push /
/// pop / pop-push sequences must produce identical popped streams.
/// (`del` has no std counterpart and is covered by the multiset oracle.)
#[test]
fn differential_vs_std_binary_heap() {
    let mut rng = Rng::new(0xb105_7d1f_f001_d00d);
    for &min_heap in &[true, false] {
        let mut less: fn(&i32, &i32) -> bool = if min_heap { less_than } else { greater_than };
        let mut ours = MinHeap::with_storage(vec![0; 512]);
        // Max-heap directly; min-heap via Reverse.
        let mut theirs_max: BinaryHeap<i32> = BinaryHeap::new();
        let mut theirs_min: BinaryHeap<Reverse<i32>> = BinaryHeap::new();

        for _ in 0..2048 {
            match rng.next_u32() % 3 {
                0 | 1 => {
                    if ours.nr() < ours.size() {
                        let v = (rng.next_u32() % 4096) as i32 - 2048;
                        assert!(ours.push(v, &mut less));
                        theirs_max.push(v);
                        theirs_min.push(Reverse(v));
                    }
                }
                2 if ours.nr() > 0 => {
                    let a = ours.pop(&mut less);
                    let b = if min_heap {
                        theirs_min.pop().map(|Reverse(x)| x)
                    } else {
                        theirs_max.pop()
                    };
                    assert_eq!(a, b, "pop streams diverged");
                }
                _ => {}
            }
            // Interleave some pop_push (std models it as pop+replace-root).
            if rng.next_u32() % 4 == 0 && ours.nr() > 0 {
                let v = (rng.next_u32() % 4096) as i32 - 2048;
                let a = ours.pop_push(v, &mut less);
                let b = if min_heap {
                    theirs_min.pop().map(|Reverse(x)| x)
                } else {
                    theirs_max.pop()
                };
                assert_eq!(a, b, "pop_push streams diverged");
                theirs_max.push(v);
                theirs_min.push(Reverse(v));
            }
        }
        while ours.nr() > 0 {
            let a = ours.pop(&mut less);
            let b = if min_heap {
                theirs_min.pop().map(|Reverse(x)| x)
            } else {
                theirs_max.pop()
            };
            assert_eq!(a, b);
        }
    }
}

/// Independent O(n^2) selection reference: drained order must equal repeated
/// linear-scan minimum extraction — no heap algorithm shared whatsoever.
#[test]
fn drained_order_matches_linear_selection_reference() {
    fn reference_drain(mut values: Vec<i32>, less: &fn(&i32, &i32) -> bool) -> Vec<i32> {
        let mut out = Vec::new();
        while !values.is_empty() {
            // Find extremum by linear scan.
            let mut best = 0;
            for i in 1..values.len() {
                if less(&values[i], &values[best]) {
                    best = i;
                }
            }
            out.push(values.remove(best));
        }
        out
    }

    let mut rng = Rng::new(0xc0ff_ee00_0000_1234);
    for &min_heap in &[true, false] {
        let mut less: fn(&i32, &i32) -> bool = if min_heap { less_than } else { greater_than };
        for n in [0usize, 1, 2, 3, 7, 16, 63, 100] {
            let vals: Vec<i32> = (0..n).map(|_| (rng.next_u32() % 50) as i32).collect();
            let mut heap = MinHeap::from_array(vals.clone());
            heapify_all(heap.as_mut_slice(), &mut less);
            let got = heap.drain_sorted(&mut less);
            let want = reference_drain(vals, &less);
            assert_eq!(got, want, "n={n} min_heap={min_heap}");
        }
    }
}

/// Full-invariant-after-every-op sweep across sizes, including heavy
/// duplicate keys (tie-break stress).
#[test]
fn invariant_after_every_op_duplicates() {
    let mut less: fn(&i32, &i32) -> bool = less_than;
    let mut rng = Rng::new(0xd05_0000_beef_f00d);
    let mut heap = MinHeap::with_storage(vec![0; 128]);
    for _ in 0..512 {
        if !heap.is_full() {
            assert!(heap.push((rng.next_u32() % 3) as i32, &mut less));
        } else {
            assert!(heap.pop(&mut less).is_some());
        }
        check_invariant(heap.as_slice(), less);
    }
}

// -----------------------------------------------------------------------
// Edge cases and API semantics
// -----------------------------------------------------------------------

#[test]
fn empty_and_full_semantics() {
    let mut less: fn(&i32, &i32) -> bool = less_than;

    let mut heap: MinHeap<i32> = MinHeap::with_storage(vec![0; 0]);
    assert!(heap.is_empty());
    assert_eq!(heap.size(), 0);
    assert_eq!(heap.pop(&mut less), None);
    assert_eq!(heap.del(0, &mut less), None);
    assert_eq!(heap.pop_push(1, &mut less), None);
    assert!(!heap.push(1, &mut less), "push onto zero-capacity heap");

    let mut heap = MinHeap::with_storage(vec![0; 2]);
    assert!(heap.peek().is_none());
    assert!(heap.push(10, &mut less));
    assert!(!heap.is_full());
    assert!(heap.push(20, &mut less));
    assert!(heap.is_full());
    assert!(!heap.push(30, &mut less), "push onto full heap must fail");

    assert_eq!(heap.del(5, &mut less), None, "out-of-range del");
    assert_eq!(heap.del(1, &mut less), Some(20));
    assert_eq!(heap.del(0, &mut less), Some(10));
    assert_eq!(heap.pop(&mut less), None);
}

#[test]
fn peek_and_len_tracking() {
    let mut less: fn(&i32, &i32) -> bool = less_than;
    let mut heap = MinHeap::with_storage(vec![0; 4]);
    for v in [5, 3, 8, 1] {
        heap.push(v, &mut less);
        // Peek must always observe the current minimum.
        let m = *heap.as_slice().iter().min().unwrap();
        assert_eq!(heap.peek(), Some(&m));
    }
    assert_eq!(heap.nr(), 4);
    assert_eq!(heap.size(), 4);
}

#[test]
fn pop_push_returns_replaced_minimum_single_sift() {
    // Semantics check: pop_push(x) removes the OLD minimum and inserts x.
    let mut less: fn(&i32, &i32) -> bool = less_than;
    let mut heap = MinHeap::from_array(vec![4, 9, 1, 7]);
    heapify_all(heap.as_mut_slice(), &mut less);
    assert_eq!(heap.peek(), Some(&1));
    assert_eq!(heap.pop_push(0, &mut less), Some(1));
    assert_eq!(heap.peek(), Some(&0));
    // Contents must be {0,4,7,9}.
    let mut got = Vec::from(heap.as_slice());
    got.sort_unstable();
    assert_eq!(got, [0, 4, 7, 9]);
}

#[test]
fn del_middle_element_keeps_consistency() {
    let mut less: fn(&i32, &i32) -> bool = less_than;
    let mut heap = MinHeap::from_array(vec![1, 5, 2, 9, 6, 3]);
    heapify_all(heap.as_mut_slice(), &mut less);
    // Delete a few middle indices; contents stay a permutation, pops ascend.
    let removed1 = heap.del(2, &mut less).unwrap();
    let removed2 = heap.del(2, &mut less).unwrap();
    check_invariant(heap.as_slice(), less);
    let drained = heap.drain_sorted(&mut less);
    let mut expect = vec![1, 5, 2, 9, 6, 3];
    expect.remove(expect.iter().position(|&v| v == removed1).unwrap());
    expect.remove(expect.iter().position(|&v| v == removed2).unwrap());
    expect.sort_unstable();
    assert_eq!(drained, expect);
}

#[test]
fn merge_transfers_and_respects_capacity() {
    let mut less: fn(&i32, &i32) -> bool = less_than;

    // Fits entirely.
    let mut dst = MinHeap::with_storage(vec![0; 8]);
    let mut src = MinHeap::from_array(vec![5, 2, 9]);
    heapify_all(src.as_mut_slice(), &mut less);
    assert_eq!(dst.merge(&mut src, &mut less), 3);
    assert!(src.is_empty());
    let drained = dst.drain_sorted(&mut less);
    assert_eq!(drained, vec![2, 5, 9]);

    // Capacity-limited: leftovers remain in src, still a consistent heap.
    let mut dst = MinHeap::with_storage(vec![0; 2]);
    let mut src = MinHeap::from_array(vec![7, 1, 4, 8, 2]);
    heapify_all(src.as_mut_slice(), &mut less);
    let moved = dst.merge(&mut src, &mut less);
    assert_eq!(moved, 2);
    assert_eq!(src.nr(), 3);
    check_invariant(src.as_slice(), less);
    assert_eq!(dst.drain_sorted(&mut less), vec![1, 2]);
    // Leftovers are the three largest.
    let rest = src.drain_sorted(&mut less);
    assert_eq!(rest, vec![4, 7, 8]);
}
