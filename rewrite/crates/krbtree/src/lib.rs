// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's `lib/rbtree.c` (+ the erase logic in
//! `include/linux/rbtree_augmented.h`) as an owning red-black tree map.
//!
//! The rebalancing code below is a *literal* translation of the C insert
//! (`__rb_insert`, Cases 1-3) and erase recoloring (`____rb_erase_color`,
//! Cases 1-4, plus erase splicing Cases 1-3 from `__rb_erase_augmented`)
//! routines; the original case numbering is preserved in the comments.
//!
//! # Representation deviation (the only structural one)
//!
//! The C `struct rb_node` packs parent pointer and color into one word
//! (`__rb_parent_color`, low bit = color). That packing exists for memory
//! density on intrusive nodes and has no observable semantics. Here nodes
//! live in an arena (`Vec`), so a node is referenced by `u32` slot index and
//! parent/color are separate fields. All other logic mirrors the C control
//! flow statement-for-statement where practical.
//!
//! Further deviations, all forced by safe Rust or the owning-map API:
//! - `WRITE_ONCE()` / RCU variants exist for lockless readers; this tree has
//!   no concurrent-access surface, so plain stores are used and
//!   `rb_replace_node_rcu()` has no counterpart.
//! - The C API is intrusive: callers embed `struct rb_node` in their structs
//!   and search with their own comparators (`rb_find`, `rb_add`,
//!   `rb_search`). This map owns `(K, V)` pairs keyed by `K: Ord`; those
//!   collapse into `get`/`get_mut`/`insert`.
//! - `rb_replace_node()` has no public equivalent: replacing the value under
//!   an existing key never moves the node, which is strictly stronger than
//!   the C pattern of linking a replacement node and rewiring pointers.
//! - Postorder iteration (`rb_first_postorder`, `rb_next_postorder`) is not
//!   part of the map API and is not provided.
//! - A mutable value iterator (`iter_mut`) is likewise omitted: yielding
//!   `&mut V` items from a materialized in-order sequence cannot be expressed
//!   through safe `Iterator` (the handles alias the arena), and this rewrite
//!   is `deny(unsafe_code)`. Per-key mutation goes through
//!   [`RBTree::get_mut`].
//!
//! # Augmented trees (`rb_augment_callbacks`) — deferred
//!
//! The task allows either an on-change hook mechanism or documented
//! deferral; this is deferred, deliberately:
//!
//! 1. In C, augmentation data lives in fields of the *caller's* struct that
//!    embeds the `rb_node`; callbacks (`propagate`/`copy`/`rotate`) patch it
//!    during mutations. Here nodes are private and owned by the map, so
//!    there is no externally visible per-node state to maintain
//!    incrementally.
//! 2. Every consumer of augmentation in this repository (interval trees,
//!    vma accounting, ...) carries its own augmentation payload; a generic
//!    callback layer added before any such consumer exists in this workspace
//!    would double the verification surface of this crate without a user.
//! 3. Escape hatch: all mutation paths are internal and iteration exposes
//!    the full contents, so an augmented variant can be layered on later
//!    without breaking this API.
//!
//! # Safety note
//!
//! Where the C code performs unconditional writes that would be memory-unsafe
//! on a NULL operand (e.g. `rb_set_parent_color(tmp1, parent, RB_BLACK)` in
//! erase-color Case 1), the invariants guarantee the operand is non-NULL; the
//! Rust translation guards such writes anyway rather than panicking, which is
//! behaviorally identical while the invariants hold (verified by tests).

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

/// Node color, mirroring `RB_RED` / `RB_BLACK`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Color {
    Red,
    Black,
}

/// Arena slot index. The C side is limited by address space; this bounds the
/// tree at `u32::MAX` nodes instead.
type NodeId = u32;

/// An arena-resident node: the intrusive `struct rb_node` plus its payload.
struct Node<K, V> {
    /// `rb_left`
    left: Option<NodeId>,
    /// `rb_right`
    right: Option<NodeId>,
    /// Parent slot; `None` for the root (C encodes this as a NULL parent).
    parent: Option<NodeId>,
    color: Color,
    key: K,
    value: V,
}

/// An owning red-black tree map rewriting `lib/rbtree.c`.
///
/// After every public method the five red-black properties hold (see the
/// property list at the top of `lib/rbtree.c`) and [`len`](RBTree::len)
/// matches the number of reachable nodes.
pub struct RBTree<K, V> {
    arena: Vec<Option<Node<K, V>>>,
    free: Vec<NodeId>,
    root: Option<NodeId>,
    len: usize,
}

impl<K, V> RBTree<K, V> {
    /// Creates an empty tree (`RB_ROOT` initialization).
    pub fn new() -> Self {
        RBTree {
            arena: Vec::new(),
            free: Vec::new(),
            root: None,
            len: 0,
        }
    }
    /// Number of key-value pairs.
    pub fn len(&self) -> usize {
        self.len
    }
    /// True if the tree holds no pairs.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    // ---- helpers mirroring static inline functions from rbtree.h /
    // ---- rbtree_augmented.h ----

    fn node(&self, id: NodeId) -> &Node<K, V> {
        self.arena[id as usize].as_ref().expect("dangling slot id")
    }
    fn node_mut(&mut self, id: NodeId) -> &mut Node<K, V> {
        self.arena[id as usize].as_mut().expect("dangling slot id")
    }
    /// `__rb_change_child(old, new, parent, root)`
    fn change_child(&mut self, old: Option<NodeId>, new: Option<NodeId>, parent: Option<NodeId>) {
        match parent {
            Some(p) => {
                let n = self.node_mut(p);
                if n.left == old {
                    n.left = new;
                } else {
                    n.right = new;
                }
            }
            None => self.root = new,
        }
    }
    /// `rb_set_parent_color(rb, p, color)`
    fn set_parent_color(&mut self, id: NodeId, parent: Option<NodeId>, color: Color) {
        let n = self.node_mut(id);
        n.parent = parent;
        n.color = color;
    }
    /// `rb_set_parent(rb, p)`; color preserved. No-op on `None` where the C
    /// callers are guarded by `if (...)`.
    fn set_parent(&mut self, id: Option<NodeId>, parent: Option<NodeId>) {
        if let Some(id) = id {
            self.node_mut(id).parent = parent;
        }
    }
    /// `rb_is_black()`: absent means a NULL leaf, which is black (property 3).
    fn is_black(&self, id: Option<NodeId>) -> bool {
        match id {
            None => true,
            Some(i) => self.node(i).color == Color::Black,
        }
    }
    fn is_red(&self, id: Option<NodeId>) -> bool {
        !self.is_black(id)
    }
    /// `__rb_rotate_set_parents(old, new, root, color)`
    fn rotate_set_parents(&mut self, old: NodeId, new: NodeId, color: Color) {
        let parent = self.node(old).parent;
        let old_color = self.node(old).color;
        let newn = self.node_mut(new);
        newn.parent = parent;
        newn.color = old_color;
        self.set_parent_color(old, Some(new), color);
        self.change_child(Some(old), Some(new), parent);
    }
    /// `rb_next`: successor slot, or `None` if `id` is the maximum.
    fn next_id(&self, mut id: NodeId) -> Option<NodeId> {
        if let Some(mut r) = self.node(id).right {
            while let Some(l) = self.node(r).left {
                r = l;
            }
            return Some(r);
        }
        loop {
            let parent = self.node(id).parent?;
            if self.node(parent).right != Some(id) {
                return Some(parent);
            }
            id = parent;
        }
    }
    fn alloc_slot(&mut self, key: K, value: V, parent: Option<NodeId>, color: Color) -> NodeId {
        let node = Node {
            left: None,
            right: None,
            parent,
            color,
            key,
            value,
        };
        match self.free.pop() {
            Some(id) => {
                self.arena[id as usize] = Some(node);
                id
            }
            None => {
                self.arena.push(Some(node));
                (self.arena.len() - 1) as NodeId
            }
        }
    }
    /// `__rb_insert(node, root, augment_rotate)` with `dummy_rotate`
    /// (augmentation deferred; see crate docs).
    ///
    /// `allow(unused_assignments)`: the C code assigns `parent = node` at the
    /// end of its Case 2 even though Case 3 always follows and breaks; the
    /// assignment is kept for structural fidelity.
    #[allow(unused_assignments)]
    fn rb_insert_color(&mut self, node: NodeId) {
        let mut node = node;
        let mut parent = self.node(node).parent;
        loop {
            /*
             * Loop invariant: node is red.
             */
            let Some(p) = parent else {
                /*
                 * The inserted node is root. Either this is the first node,
                 * or we recursed at Case 1 below and are no longer
                 * violating 4).
                 */
                self.set_parent_color(node, None, Color::Black);
                break;
            };

            /*
             * If there is a black parent, we are done. Otherwise, take some
             * corrective action as, per 4), we don't want a red root or two
             * consecutive red nodes.
             */
            if self.is_black(Some(p)) {
                break;
            }

            let gparent = self
                .node(p)
                .parent
                .expect("red parent implies red grandparent");
            let gp_right = self.node(gparent).right;

            if gp_right != Some(p) {
                /* parent == gparent->rb_left */
                if let Some(uncle) = gp_right {
                    if self.is_red(Some(uncle)) {
                        /*
                         * Case 1 - node's uncle is red (color flips).
                         *
                         *       G            g
                         *      / \          / \
                         *     p   u  -->   P   U
                         *    /            /
                         *   n            n
                         *
                         * However, since g's parent might be red, and 4)
                         * does not allow this, we need to recurse at g.
                         */
                        self.set_parent_color(uncle, Some(gparent), Color::Black);
                        self.set_parent_color(p, Some(gparent), Color::Black);
                        node = gparent;
                        parent = self.node(node).parent;
                        self.set_parent_color(node, parent, Color::Red);
                        continue;
                    }
                }

                let mut tmp = self.node(p).right;
                if tmp == Some(node) {
                    /*
                     * Case 2 - node's uncle is black and node is the
                     * parent's right child (left rotate at parent).
                     *
                     *      G             G
                     *     / \           / \
                     *    p   U  -->    n   U
                     *     \           /
                     *      n         p
                     *
                     * This still leaves us in violation of 4), the
                     * continuation into Case 3 will fix that.
                     */
                    let inner = self.node(node).left;
                    self.node_mut(p).right = inner;
                    self.set_parent(inner, Some(p));
                    self.node_mut(node).left = Some(p);
                    self.set_parent_color(p, Some(node), Color::Red);
                    parent = Some(node);
                    tmp = self.node(node).right;
                }

                /*
                 * Case 3 - node's uncle is black and node is the parent's
                 * left child (right rotate at gparent).
                 *
                 *        G           P
                 *       / \         / \
                 *      p   U  -->  n   g
                 *     /                 \
                 *    n                   U
                 *
                 * Note: C reassigned parent = node inside Case 2, so the
                 * pivot below is the updated parent.
                 */
                let pivot = parent.expect("non-root path");
                self.node_mut(gparent).left = tmp; /* == parent->rb_right */
                self.set_parent(tmp, Some(gparent));
                self.node_mut(pivot).right = Some(gparent);
                if let Some(t) = tmp {
                    self.set_parent_color(t, Some(gparent), Color::Black);
                }
                self.rotate_set_parents(gparent, pivot, Color::Red);
                break;
            } else {
                /* mirror: parent == gparent->rb_right */
                let gp_left = self.node(gparent).left;
                if let Some(uncle) = gp_left {
                    if self.is_red(Some(uncle)) {
                        /* Case 1 - color flips */
                        self.set_parent_color(uncle, Some(gparent), Color::Black);
                        self.set_parent_color(p, Some(gparent), Color::Black);
                        node = gparent;
                        parent = self.node(node).parent;
                        self.set_parent_color(node, parent, Color::Red);
                        continue;
                    }
                }

                let mut tmp = self.node(p).left;
                if tmp == Some(node) {
                    /* Case 2 - right rotate at parent */
                    let inner = self.node(node).right;
                    self.node_mut(p).left = inner;
                    self.set_parent(inner, Some(p));
                    self.node_mut(node).right = Some(p);
                    self.set_parent_color(p, Some(node), Color::Red);
                    parent = Some(node);
                    tmp = self.node(node).left;
                }

                /* Case 3 - left rotate at gparent (pivot = updated parent) */
                let pivot = parent.expect("non-root path");
                self.node_mut(gparent).right = tmp; /* == parent->rb_left */
                self.set_parent(tmp, Some(gparent));
                self.node_mut(pivot).left = Some(gparent);
                if let Some(t) = tmp {
                    self.set_parent_color(t, Some(gparent), Color::Black);
                }
                self.rotate_set_parents(gparent, pivot, Color::Red);
                break;
            }
        }
    }
    fn erase_node(&mut self, node: NodeId) -> Option<V> {
        let rebalance: Option<NodeId>;

        let node_l = self.node(node).left;
        let node_r = self.node(node).right;

        if node_l.is_none() {
            /*
             * Case 1: node to erase has at most one non-NULL child (R).
             */
            let parent = self.node(node).parent;
            let was_black = self.is_black(Some(node));
            self.change_child(Some(node), node_r, parent);
            if let Some(c) = node_r {
                let color = self.node(node).color;
                self.set_parent_color(c, parent, color);
                rebalance = None;
            } else {
                rebalance = if was_black { parent } else { None };
            }
        } else if node_r.is_none() {
            /* Still case 1, but this time the child is node->rb_left */
            let Some(c) = node_l else {
                unreachable!("this branch requires node_l = Some")
            };
            let parent = self.node(node).parent;
            let color = self.node(node).color;
            self.set_parent_color(c, parent, color);
            self.change_child(Some(node), Some(c), parent);
            rebalance = None;
        } else {
            /*
             * Two children: relink the in-order successor into node's place.
             */
            let child = node_r.unwrap();
            let mut successor = child;
            let child2;
            let mut sparent;

            let s_left = self.node(successor).left;
            if s_left.is_none() {
                /*
                 * Case 2: node's successor is its right child
                 *
                 *    (n)          (s)
                 *    / \          / \
                 *  (x) (s)  ->  (x) (c)
                 *        \
                 *        (c)
                 */
                sparent = successor;
                child2 = self.node(successor).right;
            } else {
                /*
                 * Case 3: node's successor is leftmost under node's
                 * right child subtree
                 *
                 *    (n)          (s)
                 *    / \          / \
                 *  (x) (y)  ->  (x) (y)
                 *      /            /
                 *    (p)          (p)
                 *    /            /
                 *  (s)          (c)
                 *    \
                 *    (c)
                 */
                let mut tmp = s_left;
                loop {
                    sparent = successor;
                    successor = tmp.unwrap();
                    tmp = self.node(successor).left;
                    if tmp.is_none() {
                        break;
                    }
                }
                child2 = self.node(successor).right;
                self.node_mut(sparent).left = child2;
                self.node_mut(successor).right = Some(child);
                self.set_parent(Some(child), Some(successor));
            }

            let node_left = self.node(node).left;
            self.node_mut(successor).left = node_left;
            self.set_parent(node_left, Some(successor));

            let old_parent = self.node(node).parent;
            let succ_was_black = self.is_black(Some(successor));
            self.change_child(Some(node), Some(successor), old_parent);

            if let Some(c2) = child2 {
                self.set_parent_color(c2, Some(sparent), Color::Black);
                rebalance = None;
            } else {
                rebalance = if succ_was_black { Some(sparent) } else { None };
            }
            let color = self.node(node).color;
            self.node_mut(successor).color = color;
            self.node_mut(successor).parent = old_parent;
        }

        self.len -= 1;
        let removed = self.arena[node as usize]
            .take()
            .expect("erased node exists");
        self.free.push(node);

        if let Some(rb) = rebalance {
            self.rb_erase_color(rb);
        }
        Some(removed.value)
    }
    /// `____rb_erase_color(parent, root, augment_rotate)` with
    /// `dummy_rotate`. `node` starts as NULL (first iteration).
    ///
    /// `tmp1` follows the C variable: the near nephew before Case 3 runs, or
    /// the old sibling afterwards; Case 4 recolors it.
    fn rb_erase_color(&mut self, start: NodeId) {
        let mut parent = Some(start);
        let mut node: Option<NodeId> = None;

        loop {
            /*
             * Loop invariants:
             * - node is black (or NULL on first iteration)
             * - node is not the root (parent is not NULL)
             * - All leaf paths going through parent and node have a black
             *   node count that is 1 lower than other leaf paths.
             */
            let p = parent.expect("rebalance point is never the root");

            let sibling0 = self.node(p).right;
            if node != sibling0 {
                /* node == parent->rb_left */

                let mut sibling = sibling0.expect("deficient subtree has a real sibling");
                if self.is_red(Some(sibling)) {
                    /*
                     * Case 1 - left rotate at parent
                     *
                     *     P               S
                     *    / \             / \
                     *   N   s    -->    p   Sr
                     *      / \         / \
                     *     Sl  Sr      N   Sl
                     */
                    let tmp1 = self.node(sibling).left;
                    self.node_mut(p).right = tmp1;
                    self.node_mut(sibling).left = Some(p);
                    self.set_parent_color_guarded(tmp1, Some(p), Color::Black);
                    self.rotate_set_parents(p, sibling, Color::Red);
                    sibling = tmp1.expect("red sibling has an inner child");
                }

                let mut tmp1 = self.node(sibling).right;
                if self.is_black(tmp1) {
                    let tmp2 = self.node(sibling).left;
                    if self.is_black(tmp2) {
                        /*
                         * Case 2 - sibling color flip
                         * (p could be either color here)
                         *
                         *    (p)           (p)
                         *    / \           / \
                         *   N   S    -->  N   s
                         *      / \           / \
                         *     Sl  Sr        Sl  Sr
                         *
                         * This leaves us violating 5) which can be fixed by
                         * flipping p to black if it was red, or by recursing
                         * at p. p is red when coming from Case 1.
                         */
                        self.set_parent_color(sibling, Some(p), Color::Red);
                        if self.is_red(Some(p)) {
                            self.node_mut(p).color = Color::Black;
                            break;
                        } else {
                            node = Some(p);
                            let gp = self.node(p).parent;
                            if gp.is_some() {
                                parent = gp;
                                continue;
                            }
                            break;
                        }
                    }

                    /*
                     * Case 3 - right rotate at sibling
                     * (p could be either color here)
                     *
                     *   (p)           (p)
                     *   / \           / \
                     *  N   S    -->  N   sl
                     *     / \             \
                     *    sl  Sr            S
                     *                       \
                     *                        Sr
                     *
                     * Note: p might be red, and then both p and sl are red
                     * after rotation (which breaks property 4). This is fixed
                     * in Case 4 (__rb_rotate_set_parents() sets sl the color
                     * of p and sets p RB_BLACK).
                     */
                    let sl = tmp2.unwrap();
                    let near_right = self.node(sl).right;
                    self.node_mut(sibling).left = near_right;
                    self.node_mut(sl).right = Some(sibling);
                    self.node_mut(p).right = Some(sl);
                    if let Some(n) = near_right {
                        self.set_parent_color(n, Some(sibling), Color::Black);
                    }
                    tmp1 = Some(sibling); /* C: tmp1 = sibling */
                    sibling = sl; /* C: sibling = tmp2 */
                }

                /*
                 * Case 4 - left rotate at parent + color flips
                 * (p and sl could be either color here. After rotation, p
                 * becomes black, s acquires p's color, and sl keeps its
                 * color)
                 *
                 *      (p)             (s)
                 *      / \             / \
                 *     N   S     -->   P   Sr
                 *        / \         / \
                 *      (sl) sr      N  (sl)
                 */
                let tmp2 = self.node(sibling).left;
                self.node_mut(p).right = tmp2;
                self.node_mut(sibling).left = Some(p);
                /* tmp1 is non-NULL by invariant (see crate docs). */
                self.set_parent_color_guarded(tmp1, Some(sibling), Color::Black);
                self.set_parent(tmp2, Some(p));
                self.rotate_set_parents(p, sibling, Color::Black);
                break;
            } else {
                /* mirror: node == parent->rb_right */

                let mut sibling = self
                    .node(p)
                    .left
                    .expect("deficient subtree has a real sibling");
                if self.is_red(Some(sibling)) {
                    /* Case 1 - right rotate at parent */
                    let tmp1 = self.node(sibling).right;
                    self.node_mut(p).left = tmp1;
                    self.node_mut(sibling).right = Some(p);
                    self.set_parent_color_guarded(tmp1, Some(p), Color::Black);
                    self.rotate_set_parents(p, sibling, Color::Red);
                    sibling = tmp1.expect("red sibling has an inner child");
                }

                let mut tmp1 = self.node(sibling).left;
                if self.is_black(tmp1) {
                    let tmp2 = self.node(sibling).right;
                    if self.is_black(tmp2) {
                        /* Case 2 - sibling color flip */
                        self.set_parent_color(sibling, Some(p), Color::Red);
                        if self.is_red(Some(p)) {
                            self.node_mut(p).color = Color::Black;
                            break;
                        } else {
                            node = Some(p);
                            let gp = self.node(p).parent;
                            if gp.is_some() {
                                parent = gp;
                                continue;
                            }
                            break;
                        }
                    }

                    /* Case 3 - left rotate at sibling */
                    let sr = tmp2.unwrap();
                    let near_left = self.node(sr).left;
                    self.node_mut(sibling).right = near_left;
                    self.node_mut(sr).left = Some(sibling);
                    self.node_mut(p).left = Some(sr);
                    if let Some(n) = near_left {
                        self.set_parent_color(n, Some(sibling), Color::Black);
                    }
                    tmp1 = Some(sibling); /* C: tmp1 = sibling */
                    sibling = sr; /* C: sibling = tmp2 */
                }

                /* Case 4 - right rotate at parent + color flips */
                let tmp2 = self.node(sibling).right;
                self.node_mut(p).left = tmp2;
                self.node_mut(sibling).right = Some(p);
                self.set_parent_color_guarded(tmp1, Some(sibling), Color::Black);
                self.set_parent(tmp2, Some(p));
                self.rotate_set_parents(p, sibling, Color::Black);
                break;
            }
        }
    }
    /// `rb_set_parent_color` that skips `None` operands where the C code's
    /// invariants guarantee non-NULL; documented in the crate docs.
    fn set_parent_color_guarded(
        &mut self,
        id: Option<NodeId>,
        parent: Option<NodeId>,
        color: Color,
    ) {
        if let Some(id) = id {
            self.set_parent_color(id, parent, color);
        }
    }
}

impl<K: Ord, V> RBTree<K, V> {
    // ---- lookup ----

    fn find(&self, key: &K) -> Option<NodeId> {
        let mut cur = self.root;
        while let Some(i) = cur {
            let n = self.node(i);
            let next = match key.cmp(&n.key) {
                Ordering::Less => n.left,
                Ordering::Greater => n.right,
                Ordering::Equal => return Some(i),
            };
            cur = next;
        }
        None
    }
    /// Reference to the value stored under `key`.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.find(key).map(|i| &self.node(i).value)
    }
    /// Mutable reference to the value stored under `key`. Only `V` is
    /// reachable through this handle; keys cannot be modified in place.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let i = self.find(key)?;
        Some(&mut self.node_mut(i).value)
    }
    /// True if `key` is present.
    pub fn contains_key(&self, key: &K) -> bool {
        self.find(key).is_some()
    }
    /// Smallest key (`rb_first`).
    pub fn first(&self) -> Option<(&K, &V)> {
        let mut i = self.root?;
        while let Some(l) = self.node(i).left {
            i = l;
        }
        let n = self.node(i);
        Some((&n.key, &n.value))
    }
    /// Largest key (`rb_last`).
    pub fn last(&self) -> Option<(&K, &V)> {
        let mut i = self.root?;
        while let Some(r) = self.node(i).right {
            i = r;
        }
        let n = self.node(i);
        Some((&n.key, &n.value))
    }
    /// In-order iterator over key-value pairs, ascending (`rb_first` +
    /// repeated `rb_next`).
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            tree: self,
            next: self.root.map(|r| {
                let mut i = r;
                while let Some(l) = self.node(i).left {
                    i = l;
                }
                i
            }),
        }
    }
    // ---- insertion ----

    /// Inserts `key`/`value`; if `key` was already present its value is
    /// replaced and the old value returned. The existing node stays in place
    /// (see the `rb_replace_node` note in the crate docs).
    ///
    /// Rebalancing is the literal translation of `__rb_insert()`; the C
    /// Case 1/2/3 comments are preserved below, including mirrored variants.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        // Descent to find the parent and link point (the walk in `rb_add`).
        let mut parent: Option<NodeId> = None;
        let mut link = self.root;
        let mut went_left = false;
        while let Some(cur) = link {
            parent = Some(cur);
            let cmp = key.cmp(&self.node(cur).key);
            match cmp {
                Ordering::Less => {
                    went_left = true;
                    link = self.node(cur).left;
                }
                Ordering::Greater => {
                    went_left = false;
                    link = self.node(cur).right;
                }
                Ordering::Equal => {
                    // Replace in place; no structural change, no recoloring.
                    let old = core::mem::replace(&mut self.node_mut(cur).value, value);
                    return Some(old);
                }
            }
        }

        // `rb_link_node`: attach as a red leaf.
        let id = self.alloc_slot(key, value, parent, Color::Red);
        match parent {
            None => self.root = Some(id),
            Some(p) => {
                if went_left {
                    self.node_mut(p).left = Some(id);
                } else {
                    self.node_mut(p).right = Some(id);
                }
            }
        }
        self.len += 1;
        self.rb_insert_color(id);
        None
    }
    // ---- removal ----

    /// Removes `key`, returning its value (`rb_erase`). Rebalancing follows
    /// `__rb_erase_augmented()` (splice Cases 1-3) followed by
    /// `____rb_erase_color()` (recoloring Cases 1-4), verbatim in structure.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let node = self.find(key)?;
        self.erase_node(node)
    }
}

impl<K: Ord, V> Default for RBTree<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + Ord, V: Clone> Clone for RBTree<K, V> {
    fn clone(&self) -> Self {
        let mut t = RBTree::new();
        for (k, v) in self.iter() {
            t.insert(k.clone(), v.clone());
        }
        t
    }
}

impl<K: Ord + fmt::Debug, V: fmt::Debug> fmt::Debug for RBTree<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// In-order iterator (`rb_first` + repeated `rb_next`).
pub struct Iter<'a, K, V> {
    tree: &'a RBTree<K, V>,
    next: Option<NodeId>,
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next?;
        let n = self.tree.node(id);
        self.next = self.tree.next_id(id);
        Some((&n.key, &n.value))
    }
}

#[cfg(test)]
mod tests;
