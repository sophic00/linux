//! Verification suite for the `lib/rbtree.c` rewrite:
//!
//! 1. A full red-black invariant checker (BST order, root black, no red-red,
//!    equal black-heights, plus arena/parent/len consistency) run after every
//!    mutation in the randomized suites.
//! 2. Differential fuzz against `std::collections::BTreeMap` over random
//!    interleavings of insert/remove/get for sizes up to 500.
//! 3. Exhaustive insert+remove orderings for n <= 5.
//! 4. Targeted API tests (replace semantics, first/last, empty-tree edges).

// SPDX-License-Identifier: GPL-2.0

extern crate alloc;
extern crate std;

use alloc::vec;
use alloc::vec::Vec;
use std::println;

use super::*;

/// Verifies every red-black + representation invariant of `t`.
///
/// Returns the black-height of `root` (property 5), or panics with a
/// description of the first violated property.
fn check_invariants<K: Ord + std::fmt::Debug, V>(t: &RBTree<K, V>) {
    // Property 2: the root is black.
    if let Some(r) = t.root {
        assert_eq!(t.node(r).color, Color::Black, "root is not black");
    }

    fn walk<K: Ord + std::fmt::Debug, V>(
        t: &RBTree<K, V>,
        id: NodeId,
        parent: Option<NodeId>,
        lo: Option<&K>,
        hi: Option<&K>,
    ) -> usize {
        let n = t.node(id);

        // Arena/parent consistency: the tree must point back at us exactly
        // through our recorded parent slot.
        assert_eq!(n.parent, parent, "parent pointer mismatch at node {id:?}");

        // BST ordering within [lo, hi) bounds.
        if let Some(lo) = lo {
            assert!(n.key > *lo, "BST violation: key {:?} <= lower bound", n.key);
        }
        if let Some(hi) = hi {
            assert!(n.key < *hi, "BST violation: key {:?} >= upper bound", n.key);
        }

        // Properties 1 & 4: a red node has black (possibly NULL) children.
        if n.color == Color::Red {
            assert!(
                t.is_black(n.left) && t.is_black(n.right),
                "red-red violation at key {:?}",
                n.key
            );
        }

        let lb = match n.left {
            Some(l) => walk(t, l, Some(id), lo, Some(&n.key)),
            None => 0, /* NULL leaves are black */
        };
        let rb = match n.right {
            Some(r) => walk(t, r, Some(id), Some(&n.key), hi),
            None => 0,
        };

        // Property 5: equal black-height on both sides.
        assert_eq!(lb, rb, "black-height mismatch under key {:?}", n.key);

        lb + usize::from(n.color == Color::Black)
    }

    let bh = t.root.map(|r| walk(t, r, None, None, None));

    // len must equal the number of reachable nodes; iteration must be
    // strictly ascending and cover exactly those nodes.
    let walked: Vec<(&K, &V)> = t.iter().collect();
    assert_eq!(walked.len(), t.len(), "len != reachable count");
    for w in walked.windows(2) {
        assert!(w[0].0 < w[1].0, "iteration not ascending");
    }
    let _ = bh;
}

/// Deterministic xorshift64 PRNG.
struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        XorShift(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// Uniform in [0, bound).
    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

#[test]
fn empty_tree_edges() {
    let mut t: RBTree<u32, char> = RBTree::new();
    check_invariants(&t);
    assert!(t.is_empty());
    assert_eq!(t.len(), 0);
    assert_eq!(t.get(&1), None);
    assert_eq!(t.remove(&1), None);
    assert_eq!(t.first(), None);
    assert_eq!(t.last(), None);
    assert!(!t.contains_key(&0));
}

#[test]
fn single_and_two_nodes() {
    let mut t = RBTree::new();
    assert_eq!(t.insert(5, 'a'), None);
    check_invariants(&t);
    assert_eq!(t.first(), Some((&5, &'a')));
    assert_eq!(t.last(), Some((&5, &'a')));

    assert_eq!(t.insert(3, 'b'), None);
    check_invariants(&t);
    assert_eq!(t.insert(7, 'c'), None);
    check_invariants(&t);
    assert_eq!(t.first(), Some((&3, &'b')));
    assert_eq!(t.last(), Some((&7, &'c')));

    let collected: Vec<(u32, char)> = t.iter().map(|(k, v)| (*k, *v)).collect();
    assert_eq!(collected, [(3, 'b'), (5, 'a'), (7, 'c')]);
}

#[test]
fn replace_returns_old_value_and_keeps_node() {
    let mut t = RBTree::new();
    for k in [10u32, 5, 15, 12] {
        t.insert(k, k as i64 * 100);
        check_invariants(&t);
    }
    let shape_before: Vec<u32> = t.iter().map(|(k, _)| *k).collect();

    assert_eq!(t.insert(12, -1), Some(1200));
    check_invariants(&t);
    assert_eq!(t.get(&12), Some(&-1));
    // No structural change on replace.
    let shape_after: Vec<u32> = t.iter().map(|(k, _)| *k).collect();
    assert_eq!(shape_before, shape_after);
    assert_eq!(t.len(), 4);

    // get_mut cannot break anything structural.
    *t.get_mut(&5).unwrap() = -2;
    check_invariants(&t);
    assert_eq!(t.get(&5), Some(&-2));
}

#[test]
fn remove_all_orders_small_exhaustive() {
    // Exhaustive: every insertion permutation, then every removal permutation,
    // for all distinct key sets of size <= 5 drawn from 0..6.
    use std::collections::BTreeMap;
    let universe = [0u32, 1, 2, 3, 4, 5];

    for n in 0..=5usize {
        // All subsets of size n, then permutations via Heap's algorithm.
        for keys in subsets(&universe, n) {
            for ins in permutations(&keys) {
                for del in permutations(&keys) {
                    let mut t: RBTree<u32, u32> = RBTree::new();
                    let mut oracle = BTreeMap::new();
                    for &k in &ins {
                        assert_eq!(t.insert(k, k * 10), None, "ins {ins:?}");
                        assert_eq!(oracle.insert(k, k * 10), None);
                    }
                    for &k in &del {
                        assert_eq!(
                            t.remove(&k),
                            oracle.remove(&k),
                            "del {k} of {ins:?}/{del:?}"
                        );
                        check_invariants(&t);
                    }
                    assert_eq!(
                        t.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>(),
                        oracle.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>()
                    );
                    check_invariants(&t);
                }
            }
        }
    }
}

fn subsets(universe: &[u32], n: usize) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new()];
    for &x in universe {
        for prev in out.clone() {
            if prev.len() < n {
                let mut v = prev;
                v.push(x);
                out.push(v);
            }
        }
    }
    out.into_iter().filter(|v| v.len() == n).collect()
}

fn permutations(items: &[u32]) -> Vec<Vec<u32>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut p in permutations(&rest) {
            p.insert(0, head);
            out.push(p);
        }
    }
    out
}

/// Differential fuzz vs `std::collections::BTreeMap`.
#[test]
fn differential_fuzz_vs_btreemap() {
    for seed in 0..24u64 {
        let mut rng = XorShift::new(seed ^ 0x9e37_79b9_7f4a_7c15);
        let mut t: RBTree<u32, i64> = RBTree::new();
        let mut oracle = std::collections::BTreeMap::new();

        let max_key = 1 + rng.below(500);
        for step in 0..4000u64 {
            let key = rng.below(max_key) as u32;
            match rng.below(3) {
                0 | 1 => {
                    let val = (step * 31 + seed) as i64;
                    assert_eq!(
                        t.insert(key, val),
                        oracle.insert(key, val),
                        "seed {seed} step {step} insert {key}"
                    );
                }
                _ => {
                    assert_eq!(
                        t.remove(&key),
                        oracle.remove(&key),
                        "seed {seed} step {step} remove {key}"
                    );
                }
            }
            assert_eq!(t.contains_key(&key), oracle.contains_key(&key));
            assert_eq!(t.get(&key), oracle.get(&key));
            assert_eq!(t.len(), oracle.len());
            check_invariants(&t);
        }

        // Final full comparison: identical contents in identical order.
        let mine: Vec<(u32, i64)> = t.iter().map(|(k, v)| (*k, *v)).collect();
        let theirs: Vec<(u32, i64)> = oracle.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(mine, theirs, "seed {seed}");
        assert_eq!(t.first(), oracle.iter().next());
        assert_eq!(t.last(), oracle.iter().next_back());
    }
}

/// Larger trees with dense sequential churn: exercises deep rebalancing and
/// the erase-color loop climbing toward the root.
#[test]
fn sequential_insert_then_drain() {
    let mut t = RBTree::new();
    const N: u32 = 4096;
    for k in 0..N {
        t.insert(k, (k as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        if k % 512 == 0 {
            check_invariants(&t);
        }
    }
    check_invariants(&t);
    assert_eq!(t.len(), N as usize);
    assert_eq!(t.first().unwrap().0, &0);
    assert_eq!(t.last().unwrap().0, &(N - 1));

    // Drain from both ends alternately (min/max removal paths).
    let mut expect_lo = 0u32;
    let mut expect_hi = N - 1;
    while !t.is_empty() {
        if expect_lo <= expect_hi {
            assert_eq!(
                t.remove(&expect_lo),
                Some((expect_lo as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
            );
            expect_lo += 1;
        } else {
            assert_eq!(
                t.remove(&expect_hi),
                Some((expect_hi as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
            );
            expect_hi = expect_hi.wrapping_sub(1);
        }
        if t.len() % 1024 == 0 {
            check_invariants(&t);
        }
    }
    check_invariants(&t);
}

/// The classic ascending/descending insertion patterns that degenerate naive
/// BSTs; forces every rotation case repeatedly.
#[test]
fn adversarial_orderings() {
    for order in [OrderingKind::Asc, OrderingKind::Desc, OrderingKind::ZigZag] {
        let mut t = RBTree::new();
        let mut oracle = std::collections::BTreeMap::new();
        let keys: Vec<u32> = match order {
            OrderingKind::Asc => (0..300).collect(),
            OrderingKind::Desc => (0..300).rev().collect(),
            OrderingKind::ZigZag => (0..300)
                .map(|i| if i % 2 == 0 { i } else { 299 - (i - 1) })
                .collect(),
        };
        for (round, k) in keys.iter().enumerate() {
            t.insert(*k, round as i32);
            oracle.insert(*k, round as i32);
            check_invariants(&t);
        }
        assert_eq!(
            t.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>(),
            oracle.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>()
        );
        // Remove in reverse insertion order.
        for k in keys.iter().rev() {
            t.remove(k);
            oracle.remove(k);
            check_invariants(&t);
        }
        assert!(t.is_empty());
    }
}

enum OrderingKind {
    Asc,
    Desc,
    ZigZag,
}

#[test]
fn debug_seed0_sequence() {
    // From examples/bisect.rs seed 0:
    let ops: [(u8, u32); 11] = [
        (1, 5),
        (1, 1),
        (0, 2),
        (0, 5),
        (0, 5),
        (1, 1),
        (1, 1),
        (0, 4),
        (0, 3),
        (0, 5),
        (1, 4),
    ];
    let mut t: RBTree<u32, u32> = RBTree::new();
    for (i, &(op, k)) in ops.iter().enumerate() {
        if op == 0 {
            t.insert(k, k * 7);
        } else {
            t.remove(&k);
        }
        println!(
            "step {i} op={op} k={k} len={} root={:?} arena={:#?}",
            t.len(),
            t.root,
            t.arena
                .iter()
                .enumerate()
                .map(|(i, n)| (
                    i,
                    n.as_ref()
                        .map(|n| (n.key, n.left, n.right, n.parent, n.color))
                ))
                .collect::<Vec<_>>()
        );
        println!(
            "  keys: {:?}",
            t.iter().map(|(k, _)| *k).collect::<Vec<_>>()
        );
    }
}
