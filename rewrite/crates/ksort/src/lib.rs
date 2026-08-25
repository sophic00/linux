// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's `lib/sort.c` and `lib/bsearch.c`.
//!
//! A fast, small, non-recursive O(n log n) sort. This is a faithful
//! translation of the kernel's bottom-up heapsort, which performs
//! n*log2(n) + 0.37*n + o(n) comparisons on average and
//! 1.5*n*log2(n) + O(n) in the (very contrived) worst case.
//!
//! Quicksort manages n*log2(n) - 1.26*n for random inputs (1.63*n better)
//! at the expense of stack usage and much larger code to avoid quicksort's
//! O(n^2) worst case — which is why the kernel prefers heapsort.

#![no_std]
#![deny(unsafe_code)]

use core::cmp::Ordering;

/// `sort_r()` / `__sort_r()`: bottom-up heapsort with an explicit comparator.
///
/// The comparison function must adhere to:
/// - *Antisymmetry*: `cmp(a, b)` must return the opposite ordering of
///   `cmp(b, a)`.
/// - *Transitivity*: if `cmp(a, b)` is not [`Ordering::Greater`] and
///   `cmp(b, c)` is not [`Ordering::Greater`], then `cmp(a, c)` must not be
///   [`Ordering::Greater`].
///
/// Sorting time is O(n log n) on average and worst case. The C version's
/// specialized word-swapping routines (`swap_words_32/64`, `swap_bytes`) are
/// unnecessary here: `mem::swap` on generic `T` compiles to efficient
/// wide loads/stores without any alignment dispatch, and there are no
/// retpolines to avoid.
///
/// The C API's `cond_resched()` variant (`sort_r_nonatomic`) has no
/// equivalent: scheduling is orthogonal to the algorithm and lives in the
/// kernel's thread context, not in a pure sorting routine.
pub fn heapsort_by<T, F>(v: &mut [T], mut cmp: F)
where
    F: FnMut(&T, &T) -> Ordering,
{
    let num = v.len();
    // Pre-scale counters; the C code works in byte offsets, we work in
    // element indices (i.e. "size" == 1).
    let mut n = num;
    let mut a = num / 2;
    let mut shift = 0usize;

    if a == 0 {
        return; // num < 2
    }

    /*
     * Loop invariants:
     * 1. elements [a, n) satisfy the heap property (compare greater than
     *    all of their children),
     * 2. elements [n, num) are sorted, and
     * 3. a <= b <= c <= d <= n (whenever they are valid).
     */
    loop {
        if a != 0 {
            // Building heap: sift down a.
            a -= 1usize << shift;
        } else if n > 3 {
            // Sorting: extract two largest elements.
            n -= 1;
            v.swap(0, n);
            shift = usize::from(cmp(&v[1], &v[2]) != Ordering::Greater);
            a = 1usize << shift;
            n -= 1;
            v.swap(a, n);
        } else {
            // Sort complete.
            break;
        }

        /*
         * Sift element at "a" down into heap. This is the "bottom-up"
         * variant, which significantly reduces calls to cmp(): we find the
         * sift-down path all the way to the leaves (one compare per level),
         * then backtrack to find where to insert the target element.
         */
        let mut b = a;
        let mut c;
        loop {
            c = 2 * b + 1;
            let d = c + 1;
            if d >= n {
                if d == n {
                    // Special case last leaf with no sibling.
                    b = c;
                }
                break;
            }
            b = if cmp(&v[c], &v[d]) == Ordering::Greater {
                c
            } else {
                d
            };
        }

        // Now backtrack from "b" to the correct location for "a".
        while b != a && cmp(&v[a], &v[b]) != Ordering::Less {
            b = (b - 1) / 2; // parent()
        }
        let c = b; // Where "a" belongs.
        while b != a {
            // Shift it into place.
            b = (b - 1) / 2;
            v.swap(b, c);
        }
    }

    n -= 1;
    v.swap(0, n);
    if n == 2 && cmp(&v[0], &v[1]) == Ordering::Greater {
        v.swap(0, 1);
    }
}

/// Convenience wrapper: heapsort in ascending order (`sort()` with the
/// kernel's default comparisons).
pub fn heapsort<T: Ord>(v: &mut [T]) {
    heapsort_by(v, Ord::cmp);
}

/// `bsearch()`: binary search in a sorted slice.
///
/// `key` is the item searched for; `base` must already be sorted with respect
/// to `cmp`. Returns the index of the element equal to `key`, or `None`.
pub fn bsearch_by<T, K, F>(key: &K, base: &[T], mut cmp: F) -> Option<usize>
where
    F: FnMut(&K, &T) -> Ordering,
{
    let mut lo = 0usize;
    let mut hi = base.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        match cmp(key, &base[mid]) {
            Ordering::Equal => return Some(mid),
            Ordering::Less => hi = mid,
            Ordering::Greater => lo = mid + 1,
        }
    }
    None
}

#[cfg(test)]
mod tests;
