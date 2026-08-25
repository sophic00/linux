// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's `lib/list_sort.c`.
//!
//! Bottom-up merge sort for linked lists: non-recursive, always performing
//! at least 2:1 balanced merges, with the same "pending runs" schedule as
//! the C code (each run is power-of-two sized; two runs of size 2^k are
//! merged as soon as `count` reaches an odd multiple of 2^k).
//!
//! The C `list_sort()` operates on intrusive circular doubly-linked
//! `struct list_head` nodes. Rust ownership makes an intrusive,
//! prev-pointer-threaded pending list impossible in safe code, so this
//! rewrite operates on an owned singly-linked [`List<T>`]. Deviations from
//! C, all non-observable in output order:
//!
//! - The pending sublists are held in a stack-allocated `Vec` of run heads
//!   instead of being threaded through the nodes' `prev` pointers. This
//!   costs O(log n) auxiliary space instead of O(1); the merge *schedule*
//!   (driven by the bit pattern of `count`, exactly as in C) and therefore
//!   the comparison sequence and result are identical.
//! - `merge_final()` exists in C only to rebuild the circular
//!   doubly-linked structure; there are no `prev` links here, so the final
//!   merge uses the same routine as the intermediate ones.
//!
//! Like the C version this sort is **stable**: on ties the comparator's
//! `Ordering::Equal` (or anything not `Greater`) keeps the element that
//! appeared first in the input.

extern crate alloc;

use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;
use core::iter::FromIterator;

/// A node of the owned singly-linked list.
pub struct Node<T> {
    elem: T,
    next: Option<Box<Node<T>>>,
}

/// An owned null-terminated singly-linked list, the safe-Rust counterpart
/// of a `struct list_head` chain for the purposes of [`List::list_sort`].
pub struct List<T> {
    head: Option<Box<Node<T>>>,
    len: usize,
}

impl<T> List<T> {
    /// Creates an empty list.
    pub const fn new() -> Self {
        List { head: None, len: 0 }
    }

    /// Returns `true` if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Adds an element at the front (O(1)).
    pub fn push_front(&mut self, elem: T) {
        self.head = Some(Box::new(Node {
            elem,
            next: self.head.take(),
        }));
        self.len += 1;
    }

    /// Removes and returns the first element, or `None` if empty.
    pub fn pop_front(&mut self) -> Option<T> {
        let node = self.head.take()?;
        self.head = node.next;
        self.len -= 1;
        Some(node.elem)
    }

    /// Borrows the elements in order.
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            next: self.head.as_deref(),
        }
    }

    /// Consumes the list, yielding elements in order.
    pub fn into_iter_list(self) -> IntoIter<T> {
        IntoIter { next: self.head }
    }

    /// `list_sort()`: sorts the list with a stable bottom-up merge sort
    /// using `cmp` as the comparison function.
    ///
    /// Mirrors the C semantics: `cmp(a, b)` returning anything other than
    /// [`Ordering::Greater`] means "`a` sorts before `b`, or keep their
    /// original relative order". The comparator is always called with the
    /// earlier-input element in `a`.
    pub fn list_sort<F>(&mut self, mut cmp: F)
    where
        F: FnMut(&T, &T) -> Ordering,
    {
        if self.len < 2 {
            // Zero or one elements.
            return;
        }

        // Pending runs, newest/smallest first; run at index k has size 2^k
        // whenever it is the only run of its size. Each entry is a sorted,
        // null-terminated singly-linked run.
        let mut pending: Vec<Box<Node<T>>> = Vec::new();
        let mut count: usize = 0; // Count of pending

        let mut list = self.head.take();

        loop {
            // Find the least-significant clear bit in count.
            let mut bits = count;
            let mut idx = 0usize;
            while bits & 1 == 1 {
                bits >>= 1;
                idx += 1;
            }
            // Do the indicated merge. When bits != 0, the C invariant
            // guarantees two runs exist at idx and idx+1.
            if bits != 0 {
                debug_assert!(idx + 1 < pending.len());
                let newer = pending.remove(idx);
                let older = pending.remove(idx);
                // Older run passed first: ties keep the earlier input.
                let merged = merge(Some(older), Some(newer), &mut cmp);
                pending.insert(idx, merged);
            }

            // Move one element from input list to pending (as a size-1 run).
            let mut node = list.take().expect("len invariant");
            list = node.next.take();
            pending.insert(0, node);
            count += 1;

            if list.is_none() {
                break;
            }
        }

        // End of input; merge together all the pending runs. Mirrors the C
        // final phase: start from the newest run, merge in each successively
        // older run (second entry first), with the oldest merged last.
        debug_assert!(!pending.is_empty());
        let mut acc: Box<Node<T>> = pending.remove(0);
        while pending.len() > 1 {
            let older = pending.remove(0);
            acc = merge(Some(older), Some(acc), &mut cmp);
        }
        if let Some(oldest) = pending.pop() {
            acc = merge(Some(oldest), Some(acc), &mut cmp);
        }

        self.head = Some(acc);
        debug_assert_eq!(self.len(), count);
    }
}

impl<T> Default for List<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Extend<T> for List<T> {
    /// Appends all elements, preserving iteration order.
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        // Stage the new nodes on a stack (reversed), then invert the stack
        // so we can splice an in-order chain after the current tail without
        // repeated O(n) walks.
        let mut stack: Option<Box<Node<T>>> = None;
        let mut added = 0usize;
        for elem in iter {
            stack = Some(Box::new(Node { elem, next: stack }));
            added += 1;
        }
        if added == 0 {
            return;
        }
        let mut ordered: Option<Box<Node<T>>> = None;
        while let Some(mut node) = stack {
            stack = node.next.take();
            node.next = ordered;
            ordered = Some(node);
        }
        let mut tail = &mut self.head;
        while let Some(node) = tail {
            tail = &mut node.next;
        }
        *tail = ordered;
        self.len += added;
    }
}

impl<T> FromIterator<T> for List<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut l = List::new();
        l.extend(iter);
        l
    }
}

impl<T: fmt::Debug> fmt::Debug for List<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Borrowing iterator over [`List`].
pub struct Iter<'a, T> {
    next: Option<&'a Node<T>>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.next?;
        self.next = node.next.as_deref();
        Some(&node.elem)
    }
}

/// Consuming iterator over [`List`].
pub struct IntoIter<T> {
    next: Option<Box<Node<T>>>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let mut node = self.next.take()?;
        self.next = node.next.take();
        Some(node.elem)
    }
}

/// `merge()`: merges two sorted, null-terminated runs into one sorted run.
///
/// On ties (`cmp <= 0`) the element from `a` is taken first — important for
/// stability, since every call site passes the earlier-input run as `a`,
/// exactly like the C code (`merge(priv, cmp, b, a)` in the main loop,
/// `merge(pending, list)` / `merge_final(head, pending, list)` at the end).
fn merge<T, F>(mut a: Option<Box<Node<T>>>, mut b: Option<Box<Node<T>>>, mut cmp: F) -> Box<Node<T>>
where
    F: FnMut(&T, &T) -> Ordering,
{
    debug_assert!(a.is_some() && b.is_some());
    let mut head: Option<Box<Node<T>>> = None;
    // Tail pointer into the growing result, walked down link by link.
    let mut tail = &mut head;

    loop {
        /* if equal, take 'a' -- important for sort stability */
        let take_a = match (&a, &b) {
            (Some(x), Some(y)) => cmp(&x.elem, &y.elem) != Ordering::Greater,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let src = if take_a { &mut a } else { &mut b };
        let mut node = src.take().unwrap();
        *src = node.next.take();
        *tail = Some(node);
        tail = &mut tail.as_mut().unwrap().next;
    }

    head.expect("at least one input run was non-empty")
}

#[cfg(test)]
mod tests;
