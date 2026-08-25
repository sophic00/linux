// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's `lib/min_heap.c` and its header
//! `include/linux/min_heap.h` — a capacity-bounded binary min-heap.
//!
//! # C-to-Rust correspondence
//!
//! | C (min_heap.h / min_heap.c)                    | Rust                                        |
//! |------------------------------------------------|---------------------------------------------|
//! | `MIN_HEAP_PREALLOCATED` / `DEFINE_MIN_HEAP`, `min_heap_char` | [`MinHeap<T>`] (owned storage, fixed capacity) |
//! | `min_heap_init(_inline)`                       | [`MinHeap::with_storage`]                   |
//! | (the tests' `.data = v, .nr = .size = n` idiom)| [`MinHeap::from_array`]                     |
//! | `min_heap_peek(_inline)`                       | [`MinHeap::peek`] / [`peek_slice`]          |
//! | `min_heap_full_inline`                         | [`MinHeap::is_full`]                        |
//! | `__min_heap_sift_down_inline`                  | [`sift_down`]                               |
//! | `__min_heap_sift_up_inline`                    | [`sift_up`]                                 |
//! | `__min_heapify_all_inline` (Floyd, O(nr))      | [`heapify_all`]                             |
//! | `__min_heap_pop_inline`                        | [`MinHeap::pop`]                            |
//! | `__min_heap_pop_push_inline`                   | [`MinHeap::pop_push`]                       |
//! | `__min_heap_push_inline`                       | [`MinHeap::push`]                           |
//! | `__min_heap_del_inline`                        | [`MinHeap::del`]                            |
//! | `struct min_heap_callbacks::less`              | `less: FnMut(&T, &T) -> bool`               |
//! | `parent(i, lsbit, size)`                       | `(i - 1) / 2`                               |
//!
//! The out-of-line `__min_heap_*()` copies in `lib/min_heap.c` are collapsed
//! into these functions: Rust monomorphization makes the inline/out-of-line
//! distinction unnecessary.
//!
//! # Deviations from C (none observable through valid use)
//!
//! - *Custom swap callbacks dropped.* `min_heap_callbacks.swp` and the
//!   `swap_words_32/64` / `swap_bytes` / `select_swap_func` / `do_swap`
//!   machinery exist to memcpy raw bytes efficiently and to let callers fix up
//!   auxiliary pointers on swap. Safe generic Rust gets optimal wide swaps
//!   from `mem::swap`, and there are no raw auxiliary pointers to keep in
//!   sync — the same reasoning as the workspace's `ksort` crate. Callers that
//!   need per-swap side effects can wrap this type.
//! - *Error handling.* C returns `bool` and fires `WARN_ONCE` when popping an
//!   empty heap or pushing a full one; here [`MinHeap::pop`] returns `None`,
//!   [`MinHeap::del`] returns `None`, and [`MinHeap::push`] returns `false`,
//!   without panicking or warning infrastructure.
//! - *`pop_push`/`del` misuse.* In C, `pop_push` on an empty heap overwrites
//!   slot 0 past `nr` (silent corruption) and `del` with an out-of-range
//!   index reads out of bounds. Both return `None` here instead. For valid
//!   arguments the behavior is identical.
//! - *Tie-breaking preserved.* Where `less(a, b)` is false for equal
//!   elements, the algorithms below break ties exactly as the C code does
//!   (left child preferred in `sift_down`, stop-on-`!less` in the backtrack),
//!   so resulting heap layouts match the C implementation step for step.
//! - *`merge` is an extension* not present in `lib/min_heap.c`, built
//!   strictly from C `push`/`pop` semantics, provided because the task
//!   specification asks for it.
//! - The `parent()` bit-trick exists only because C tracks byte offsets;
//!   with element indices `(i - 1) / 2` is exact.

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

/// Free-function layer mirroring the `__min_heap_*_inline` algorithms. They
/// operate on the *live region* of a heap: `&mut [T]` corresponds to
/// `heap->data` restricted to `nr` elements.
///
/// Sift the element at `pos` down the heap (`__min_heap_sift_down_inline`).
///
/// Find the sift-down path all the way to the leaves (one `less()` call per
/// level), then backtrack to find where the target belongs, shifting the
/// path elements up into place — the bottom-up variant that minimizes
/// comparisons.
pub fn sift_down<T, F>(v: &mut [T], pos: usize, mut less: F)
where
    F: FnMut(&T, &T) -> bool,
{
    let n = v.len();
    let mut b = pos;
    let mut c;
    loop {
        c = 2 * b + 1;
        let d = c + 1;
        if d >= n {
            if d == n {
                // Special case for the last leaf with no sibling.
                b = c;
            }
            break;
        }
        b = if less(&v[c], &v[d]) { c } else { d };
    }

    // Backtrack to the correct location.
    while b != pos && less(&v[pos], &v[b]) {
        b = (b - 1) / 2; // parent()
    }

    // Shift the element into its correct place.
    let target = b;
    while b != pos {
        b = (b - 1) / 2;
        v.swap(b, target);
    }
}

/// Sift the element at `idx` up towards the root, O(log2(nr))
/// (`__min_heap_sift_up_inline`).
pub fn sift_up<T, F>(v: &mut [T], idx: usize, mut less: F)
where
    F: FnMut(&T, &T) -> bool,
{
    let mut a = idx;
    while a != 0 {
        let parent = (a - 1) / 2;
        if less(&v[parent], &v[a]) {
            break;
        }
        v.swap(a, parent);
        a = parent;
    }
}

/// Floyd's approach to heapification, O(nr): sift down every internal node,
/// bottom-up (`__min_heapify_all_inline`).
pub fn heapify_all<T, F>(v: &mut [T], less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    for i in (0..v.len() / 2).rev() {
        sift_down(v, i, &mut *less);
    }
}

/// Get the minimum element from the heap (`min_heap_peek`).
pub fn peek_slice<T>(v: &[T]) -> Option<&T> {
    v.first()
}

/// Capacity-bounded binary min-heap over owned storage — the Rust equivalent
/// of `DEFINE_MIN_HEAP(_type, _name)` with its preallocated array.
///
/// Invariant: `buf` holds exactly the live elements in heap order under
/// `less` (root = minimum); `cap` is the fixed storage bound (C field
/// `size`).
#[derive(Debug)]
pub struct MinHeap<T> {
    buf: Vec<T>,
    cap: usize,
}

impl<T> MinHeap<T> {
    /// `min_heap_init`: adopt `storage`'s length as the capacity with an
    /// empty heap. The bound is fixed from this point on, exactly like the C
    /// preallocated array.
    pub fn with_storage(storage: Vec<T>) -> Self {
        let cap = storage.len();
        MinHeap {
            buf: Vec::with_capacity(cap),
            cap,
        }
    }

    /// Init-from-array: the array becomes both the capacity *and* the live
    /// contents (`size == nr == len`), ready for [`heapify_all`]. This is the
    /// idiom the KUnit suite spells `.data = values, .nr = .size =
    /// ARRAY_SIZE(values)`.
    pub fn from_array(values: Vec<T>) -> Self {
        let cap = values.len();
        MinHeap { buf: values, cap }
    }

    /// Number of live elements (C field `nr`).
    pub fn nr(&self) -> usize {
        self.buf.len()
    }

    /// Storage capacity (C field `size`).
    pub fn size(&self) -> usize {
        self.cap
    }

    /// `min_heap_full`: true when no further [`MinHeap::push`] can succeed.
    pub fn is_full(&self) -> bool {
        self.buf.len() == self.cap
    }

    /// True when the heap holds no elements.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Live region as a slice (`heap->data[..heap->nr]`). Order within it is
    /// heap order, not sorted order.
    pub fn as_slice(&self) -> &[T] {
        &self.buf
    }

    /// Mutable live region for driving the free-function algorithms
    /// ([`heapify_all`], [`sift_down`], [`sift_up`]) directly on the heap's
    /// storage, the way the C `_inline` macros operate on the struct in
    /// place.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.buf
    }

    /// `min_heap_peek`: borrow of the minimum element, if any.
    pub fn peek(&self) -> Option<&T> {
        peek_slice(&self.buf)
    }

    /// `min_heap_push`: push an element on to the heap, O(log2(nr)).
    ///
    /// Returns `false` (leaving the heap unchanged) when full — the C
    /// `WARN_ONCE("Pushing on a full heap")` case, minus the warning.
    pub fn push<F>(&mut self, element: T, less: &mut F) -> bool
    where
        F: FnMut(&T, &T) -> bool,
    {
        if self.buf.len() >= self.cap {
            return false;
        }
        // Place at the end of data, then sift the child at pos up.
        self.buf.push(element);
        let pos = self.buf.len() - 1;
        sift_up(&mut self.buf, pos, &mut *less);
        true
    }

    /// `min_heap_pop`: remove and return the minimum element, O(log2(nr)).
    ///
    /// Returns `None` on an empty heap (C warns and returns false).
    pub fn pop<F>(&mut self, less: &mut F) -> Option<T>
    where
        F: FnMut(&T, &T) -> bool,
    {
        if self.buf.is_empty() {
            return None;
        }
        let last = self.buf.len() - 1;
        // Place last element at the root (position 0); the minimum rides at
        // the end, out of the sifted region, and comes off below. This is the
        // swap-based spelling of C's `memcpy(data, data + --nr * esize)`.
        self.buf.swap(0, last);
        sift_down(&mut self.buf[..last], 0, &mut *less);
        self.buf.pop()
    }

    /// `min_heap_pop_push`: replace the minimum with `element` and restore
    /// the heap property with a single sift, O(log2(nr)) — cheaper than a
    /// pop followed by a push (two sifts).
    ///
    /// Returns the removed minimum, or `None` on an empty heap (C would
    /// silently overwrite slot 0 of an empty heap instead).
    pub fn pop_push<F>(&mut self, element: T, less: &mut F) -> Option<T>
    where
        F: FnMut(&T, &T) -> bool,
    {
        if self.buf.is_empty() {
            return None;
        }
        let old = core::mem::replace(&mut self.buf[0], element);
        sift_down(&mut self.buf, 0, &mut *less);
        Some(old)
    }

    /// `min_heap_del`: remove and return the element at live-index `idx`,
    /// O(log2(nr)): move the last element into `idx`, then sift up *and* down
    /// from there (the deleted element may have been either smaller or larger
    /// than its neighbours).
    ///
    /// Returns `None` if the heap is empty or `idx >= nr` (C has no bounds
    /// check and would read out of bounds).
    pub fn del<F>(&mut self, idx: usize, less: &mut F) -> Option<T>
    where
        F: FnMut(&T, &T) -> bool,
    {
        if idx >= self.buf.len() {
            return None;
        }
        let last = self.buf.len() - 1;
        if idx == last {
            return self.buf.pop();
        }
        self.buf.swap(idx, last);
        sift_up(&mut self.buf[..last], idx, &mut *less);
        sift_down(&mut self.buf[..last], idx, &mut *less);
        self.buf.pop()
    }

    /// Extension (not in `lib/min_heap.c`): drain `other` into `self` via
    /// repeated [`MinHeap::pop`] + [`MinHeap::push`], stopping when `self`
    /// reaches capacity.
    ///
    /// Returns the number of elements transferred; elements that did not fit
    /// remain in `other`.
    pub fn merge<F>(&mut self, other: &mut MinHeap<T>, less: &mut F) -> usize
    where
        F: FnMut(&T, &T) -> bool,
    {
        let mut moved = 0;
        while !self.is_full() && !other.is_empty() {
            let el = other.pop(&mut *less).expect("non-empty checked above");
            self.push(el, &mut *less);
            moved += 1;
        }
        moved
    }

    /// Drain the heap: repeatedly [`MinHeap::pop`], yielding elements in
    /// ascending order under `less` — the `pop_verify_heap` pattern of the
    /// KUnit suite, as an API.
    pub fn drain_sorted<F>(mut self, less: &mut F) -> Vec<T>
    where
        F: FnMut(&T, &T) -> bool,
    {
        let mut out = Vec::with_capacity(self.buf.len());
        while let Some(el) = self.pop(&mut *less) {
            out.push(el);
        }
        out
    }
}

#[cfg(test)]
mod tests;
