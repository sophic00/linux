// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's `lib/bitmap.c` core operations and
//! `lib/find_bit.c` (the find_bit family), on an owned [`Bitmap`] type.
//!
//! C-to-Rust correspondence (`BITS_PER_LONG` == 64 here; bit `i` of word `w`
//! is position `w * 64 + i`, matching little-endian Linux):
//!
//! | C                                   | Rust                                   |
//! |-------------------------------------|----------------------------------------|
//! | `BITMAP_LAST_WORD_MASK(n)`          | [`last_word_mask`]                     |
//! | `BITMAP_FIRST_WORD_MASK(start)`     | [`first_word_mask`]                    |
//! | `bitmap_zero()` / `bitmap_fill()`   | [`Bitmap::zeros`] / [`Bitmap::filled`] |
//! | `bitmap_copy()`                     | [`Bitmap::copy_from`]                  |
//! | `bitmap_set()` / `__bitmap_set()`   | [`Bitmap::set_region`]                 |
//! | `bitmap_clear()` / `__bitmap_clear()` | [`Bitmap::clear_region`]             |
//! | `test_bit()` / `__set_bit()` etc.   | [`Bitmap::get`] / [`Bitmap::set`] / [`Bitmap::clear`] / [`Bitmap::assign`] |
//! | `__bitmap_and()`                    | [`Bitmap::and`]                        |
//! | `__bitmap_or()`                     | [`Bitmap::or_with`]                    |
//! | `__bitmap_xor()`                    | [`Bitmap::xor_with`]                   |
//! | `__bitmap_andnot()`                 | [`Bitmap::andnot`]                     |
//! | `__bitmap_complement()`             | [`Bitmap::complement`]                 |
//! | `__bitmap_replace()`                | [`Bitmap::replace`]                    |
//! | `__bitmap_equal()` / `bitmap_equal()` | [`Bitmap::equal`]                    |
//! | `__bitmap_or_equal()`               | [`Bitmap::or_equal`]                   |
//! | `__bitmap_intersects()`             | [`Bitmap::intersects`]                 |
//! | `__bitmap_subset()` / `bitmap_subset()` | [`Bitmap::subset`]                 |
//! | `bitmap_empty()` / `bitmap_full()`  | [`Bitmap::is_empty`] / [`Bitmap::is_full`] |
//! | `__bitmap_weight()` / `bitmap_weight()` | [`Bitmap::weight`]                 |
//! | `__bitmap_weight_and()`             | [`Bitmap::weight_and`]                 |
//! | `__bitmap_weight_andnot()`          | [`Bitmap::weight_andnot`]              |
//! | `bitmap_weighted_or()`              | [`Bitmap::weighted_or`]                |
//! | `bitmap_weighted_xor()`             | [`Bitmap::weighted_xor`]               |
//! | `bitmap_weight_from()`              | [`Bitmap::weight_from`]                |
//! | `__bitmap_shift_right()`            | [`Bitmap::shift_right`]                |
//! | `__bitmap_shift_left()`             | [`Bitmap::shift_left`]                 |
//! | `bitmap_cut()`                      | [`Bitmap::cut`]                        |
//! | `bitmap_read()` / `bitmap_write()`  | [`Bitmap::read`] / [`Bitmap::write`]   |
//! | `bitmap_get_value8()`/`_set_value8()` | [`Bitmap::get_value8`] / [`Bitmap::set_value8`] |
//! | `_find_first_bit()`                 | [`find_first_bit`]                     |
//! | `_find_first_zero_bit()`            | [`find_first_zero_bit`]                |
//! | `_find_first_and_bit()`             | [`find_first_and_bit`]                 |
//! | `_find_first_andnot_bit()`          | [`find_first_andnot_bit`]              |
//! | `_find_first_and_and_bit()`         | [`find_first_and_and_bit`]             |
//! | `_find_next_bit()`                  | [`find_next_bit`]                      |
//! | `_find_next_zero_bit()`             | [`find_next_zero_bit`]                 |
//! | `_find_next_and_bit()`              | [`find_next_and_bit`]                  |
//! | `_find_next_andnot_bit()`           | [`find_next_andnot_bit`]               |
//! | `_find_next_or_bit()`               | [`find_next_or_bit`]                   |
//! | `__find_nth_bit()`                  | [`find_nth_bit`]                       |
//! | `__find_nth_and_bit()`              | [`find_nth_and_bit`]                   |
//! | `__find_nth_and_andnot_bit()`       | [`find_nth_and_andnot_bit`]         |
//! | `_find_last_bit()`                  | [`find_last_bit`]                      |
//! | `fns()`                             | [`fns`]                                |
//! | `find_next_clump8()`                | [`find_next_clump8`]                   |
//! | `bitmap_find_next_zero_area_off()`  | [`find_next_zero_area_off`]            |
//!
//! # Faithfulness notes / deviations from C
//!
//! - The C API operates on caller-allocated `unsigned long *` arrays with a
//!   separate `nbits`; here the owned [`Bitmap`] carries `nbits` alongside its
//!   words. Slice-based free functions mirror the raw C primitives.
//! - Kernel convention keeps trailing bits (from `nbits` to end of the last
//!   word, cf. `bitmap_copy_clear_tail()`) zero; every [`Bitmap`] mutator
//!   maintains that invariant. Raw C code such as `__bitmap_or()` can smear
//!   garbage into the padding when inputs violate it — that garbage is
//!   unobservable under the invariant, and Rust's type makes it impossible
//!   to violate rather than merely discouraged.
//! - `bitmap_set()`/`bitmap_clear()` in C write into padding if
//!   `start + nbits` exceeds the declared size (callers must not do that).
//!   [`Bitmap::set_region`]/[`Bitmap::clear_region`] instead clamp the region
//!   to `[start, self.nbits)`.
//! - Shifts by more than `nbits` produce an all-zero bitmap (the C loops
//!   degenerate to that for valid callers); result tails are re-masked.
//! - `bitmap_read()/bitmap_write()` in C require the caller's storage to be
//!   large enough; [`Bitmap::write`] grows the word vector on demand instead.
//!   `nbits > 64` reads return 0 / writes do nothing, as in C.

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

/// Bits per word: `BITS_PER_LONG` on a 64-bit target.
pub const BITS_PER_LONG: usize = 64;

/// Number of words needed for `nbits` bits: `BITS_TO_LONGS(nbits)`.
#[inline]
pub const fn bits_to_longs(nbits: usize) -> usize {
    nbits.div_ceil(BITS_PER_LONG)
}

/// `BITMAP_LAST_WORD_MASK(nbits)`: mask of the low `nbits % 64` bits of a
/// word. Note the C quirk preserved here: for `nbits % 64 == 0` (including
/// 0), the mask is all ones.
#[inline]
pub const fn last_word_mask(nbits: usize) -> u64 {
    // C: `~0UL >> ((-nbits) & (BITS_PER_LONG - 1))`. Shift amount is in
    // [0, 63]; for nbits%64==0 it is 0 and the mask is all ones (C quirk).
    (!0u64) >> ((BITS_PER_LONG - nbits % BITS_PER_LONG) % BITS_PER_LONG)
}

/// `BITMAP_FIRST_WORD_MASK(start)`: mask of bits `start % 64..64` of a word.
#[inline]
pub const fn first_word_mask(start: usize) -> u64 {
    !0u64 << (start & (BITS_PER_LONG - 1))
}

/// `__ffs(word)`: index of the least significant set bit; word must be nonzero.
/// (For `word == 0` this would be undefined behaviour in C.)
#[inline]
fn ffs_nonzero(word: u64) -> usize {
    debug_assert!(word != 0);
    word.trailing_zeros() as usize
}

/// `fns(word, n)`: position of the `n`-th set bit (0-indexed) in `word`,
/// or `BITS_PER_LONG` if the word has at most `n` set bits.
#[inline]
pub fn fns(mut word: u64, n: usize) -> usize {
    let mut n = n;
    while word != 0 && n > 0 {
        // C: `while (word && n--) word &= word - 1;`
        n -= 1;
        word &= word - 1;
    }
    if word != 0 {
        ffs_nonzero(word)
    } else {
        BITS_PER_LONG
    }
}

/// An owned bitmap of `nbits` bits stored in 64-bit words, with the kernel's
/// trailing-bits-zero invariant enforced by construction.
#[derive(Clone, PartialEq, Eq)]
pub struct Bitmap {
    words: Vec<u64>,
    nbits: usize,
}

impl core::fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Bitmap")
            .field("nbits", &self.nbits)
            .field("words", &self.words)
            .finish()
    }
}

impl Bitmap {
    /// `bitmap_zero()`: a bitmap of `nbits` cleared bits.
    pub fn zeros(nbits: usize) -> Self {
        Bitmap { words: vec![0; bits_to_longs(nbits)], nbits }
    }

    /// `bitmap_fill()`: a bitmap of `nbits` set bits.
    pub fn filled(nbits: usize) -> Self {
        let mut b = Self::zeros(nbits);
        b.fill();
        b
    }

    /// Build from raw words, masking anything beyond `nbits`
    /// (as `bitmap_copy_clear_tail()` does).
    pub fn from_words_masked(words: Vec<u64>, nbits: usize) -> Self {
        let mut b = Bitmap { words, nbits };
        b.mask_tail();
        b
    }

    /// The declared size in bits.
    #[inline]
    pub fn len_bits(&self) -> usize {
        self.nbits
    }

    /// Raw word slice (trailing bits of the last word are guaranteed zero).
    #[inline]
    pub fn as_words(&self) -> &[u64] {
        &self.words
    }

    #[inline]
    fn mask_tail(&mut self) {
        let rem = self.nbits % BITS_PER_LONG;
        if rem != 0 {
            if let Some(last) = self.words.last_mut() {
                *last &= last_word_mask(self.nbits);
            }
        }
    }

    #[inline]
    fn assert_same_size(&self, other: &Bitmap) {
        assert_eq!(
            self.nbits, other.nbits,
            "bitmap size mismatch: {} vs {}",
            self.nbits, other.nbits
        );
    }

    /// `test_bit()`.
    #[inline]
    pub fn get(&self, bit: usize) -> bool {
        assert!(bit < self.nbits);
        self.words[bit / BITS_PER_LONG] & (1u64 << (bit % BITS_PER_LONG)) != 0
    }

    /// `__set_bit()`.
    #[inline]
    pub fn set(&mut self, bit: usize) {
        assert!(bit < self.nbits);
        self.words[bit / BITS_PER_LONG] |= 1u64 << (bit % BITS_PER_LONG);
    }

    /// `__clear_bit()`.
    #[inline]
    pub fn clear(&mut self, bit: usize) {
        assert!(bit < self.nbits);
        self.words[bit / BITS_PER_LONG] &= !(1u64 << (bit % BITS_PER_LONG));
    }

    /// `__assign_bit()`.
    #[inline]
    pub fn assign(&mut self, bit: usize, value: bool) {
        if value {
            self.set(bit)
        } else {
            self.clear(bit)
        }
    }

    /// Set every bit in `[start, start + nbits)` (clamped to the bitmap),
    /// like `bitmap_set()`.
    pub fn set_region(&mut self, start: usize, nbits: usize) {
        let end = (start + nbits).min(self.nbits);
        for bit in start..end {
            self.set(bit);
        }
    }

    /// Clear every bit in `[start, start + nbits)` (clamped to the bitmap),
    /// like `bitmap_clear()`.
    pub fn clear_region(&mut self, start: usize, nbits: usize) {
        let end = (start + nbits).min(self.nbits);
        for bit in start..end {
            self.clear(bit);
        }
    }

    /// `bitmap_fill()`: set all `nbits` bits.
    pub fn fill(&mut self) {
        for w in self.words.iter_mut() {
            *w = !0u64;
        }
        self.mask_tail();
    }

    /// `bitmap_zero()`: clear all `nbits` bits (size unchanged).
    pub fn make_zero(&mut self) {
        for w in self.words.iter_mut() {
            *w = 0;
        }
    }

    /// C `bitmap_zero(map, nbits)` applied to this storage: clear every word
    /// covering `[0, nbits)`, including any padding beyond `nbits`.
    /// Requires `nbits <= self.nbits`.
    pub fn zero_covering(&mut self, nbits: usize) {
        assert!(nbits <= self.nbits);
        for w in self.words[..bits_to_longs(nbits)].iter_mut() {
            *w = 0;
        }
    }

    /// C `bitmap_fill(map, nbits)` applied to this storage: set every word
    /// covering `[0, nbits)`, then restore the trailing-bits-zero invariant.
    /// Requires `nbits <= self.nbits`.
    pub fn fill_covering(&mut self, nbits: usize) {
        assert!(nbits <= self.nbits);
        for w in self.words[..bits_to_longs(nbits)].iter_mut() {
            *w = !0u64;
        }
        self.mask_tail();
    }

    /// `bitmap_copy()`.
    pub fn copy_from(&mut self, src: &Bitmap) {
        self.assert_same_size(src);
        self.words.copy_from_slice(&src.words);
    }

    /// `__bitmap_and()` / `bitmap_and()`. Returns true if the result is nonzero.
    /// Panics if `dst` aliases `src` in the C sense — here `self` is `dst`.
    pub fn and(&mut self, src1: &Bitmap, src2: &Bitmap) -> bool {
        self.assert_same_size(src1);
        src1.assert_same_size(src2);
        let mut result = 0u64;
        for k in 0..self.words.len() {
            self.words[k] = src1.words[k] & src2.words[k];
            result |= self.words[k];
        }
        result != 0
    }

    /// `__bitmap_or()` / `bitmap_or()`.
    pub fn or_with(&mut self, src1: &Bitmap, src2: &Bitmap) {
        self.assert_same_size(src1);
        src1.assert_same_size(src2);
        for k in 0..self.words.len() {
            self.words[k] = src1.words[k] | src2.words[k];
        }
        self.mask_tail();
    }

    /// `__bitmap_xor()` / `bitmap_xor()`.
    pub fn xor_with(&mut self, src1: &Bitmap, src2: &Bitmap) {
        self.assert_same_size(src1);
        src1.assert_same_size(src2);
        for k in 0..self.words.len() {
            self.words[k] = src1.words[k] ^ src2.words[k];
        }
        self.mask_tail();
    }

    /// `__bitmap_andnot()` / `bitmap_andnot()`. Returns true if the result
    /// is nonzero.
    pub fn andnot(&mut self, src1: &Bitmap, src2: &Bitmap) -> bool {
        self.assert_same_size(src1);
        src1.assert_same_size(src2);
        let mut result = 0u64;
        for k in 0..self.words.len() {
            self.words[k] = src1.words[k] & !src2.words[k];
            result |= self.words[k];
        }
        self.mask_tail();
        result != 0
    }

    /// `__bitmap_complement()` / `bitmap_complement()`.
    pub fn complement(&mut self, src: &Bitmap) {
        self.assert_same_size(src);
        for k in 0..self.words.len() {
            self.words[k] = !src.words[k];
        }
        self.mask_tail();
    }

    /// `__bitmap_replace()`: `dst = (old & ~mask) | (new & mask)`.
    pub fn replace(&mut self, old: &Bitmap, new: &Bitmap, mask: &Bitmap) {
        self.assert_same_size(old);
        old.assert_same_size(new);
        new.assert_same_size(mask);
        for k in 0..self.words.len() {
            self.words[k] = (old.words[k] & !mask.words[k]) | (new.words[k] & mask.words[k]);
        }
        self.mask_tail();
    }

    /// `bitmap_equal()` / `__bitmap_equal()`.
    pub fn equal(&self, other: &Bitmap) -> bool {
        if self.nbits != other.nbits {
            return false;
        }
        self.words == other.words
    }

    /// `bitmap_or_equal()`: `(*src1 | *src2) == *src3`.
    pub fn or_equal(&self, src2: &Bitmap, src3: &Bitmap) -> bool {
        self.assert_same_size(src2);
        src2.assert_same_size(src3);
        for k in 0..self.words.len() {
            if (self.words[k] | src2.words[k]) != src3.words[k] {
                return false;
            }
        }
        true
    }

    /// `bitmap_intersects()` / `__bitmap_intersects()`.
    pub fn intersects(&self, other: &Bitmap) -> bool {
        self.assert_same_size(other);
        for k in 0..self.words.len() {
            if self.words[k] & other.words[k] != 0 {
                return true;
            }
        }
        false
    }

    /// `bitmap_subset()` / `__bitmap_subset()`: is `self` a subset of `other`?
    pub fn subset(&self, other: &Bitmap) -> bool {
        self.assert_same_size(other);
        for k in 0..self.words.len() {
            if self.words[k] & !other.words[k] != 0 {
                return false;
            }
        }
        true
    }

    /// `bitmap_empty()`.
    pub fn is_zero(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    /// `bitmap_full()`.
    pub fn is_full(&self) -> bool {
        let rem = self.nbits % BITS_PER_LONG;
        for (k, &w) in self.words.iter().enumerate() {
            if k + 1 == self.words.len() && rem != 0 {
                if w != last_word_mask(self.nbits) {
                    return false;
                }
            } else if w != !0u64 {
                return false;
            }
        }
        true
    }

    /// `bitmap_weight()` / `__bitmap_weight()`.
    pub fn weight(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// `bitmap_weight_and()` / `__bitmap_weight_and()`.
    pub fn weight_and(&self, src2: &Bitmap) -> usize {
        self.assert_same_size(src2);
        self.words
            .iter()
            .zip(&src2.words)
            .map(|(a, b)| (a & b).count_ones() as usize)
            .sum()
    }

    /// `bitmap_weight_andnot()` / `__bitmap_weight_andnot()`.
    pub fn weight_andnot(&self, src2: &Bitmap) -> usize {
        self.assert_same_size(src2);
        self.words
            .iter()
            .zip(&src2.words)
            .map(|(a, b)| (a & !b).count_ones() as usize)
            .sum()
    }

    /// `bitmap_weighted_or()`: `*dst = *src1 | *src2`, returns result weight.
    pub fn weighted_or(&mut self, src1: &Bitmap, src2: &Bitmap) -> usize {
        self.or_with(src1, src2);
        self.weight()
    }

    /// `bitmap_weighted_xor()`: `*dst = *src1 ^ *src2`, returns result weight.
    pub fn weighted_xor(&mut self, src1: &Bitmap, src2: &Bitmap) -> usize {
        self.xor_with(src1, src2);
        self.weight()
    }

    /// `bitmap_weight_from(bitmap, start, end)`: number of set bits in
    /// `[start, end)`. If `start >= end`, returns `end` (C semantics).
    /// Bits at positions `>= self.nbits` count as clear, matching the
    /// trailing-bits-zero invariant.
    pub fn weight_from(&self, start: usize, end: usize) -> usize {
        if start >= end {
            return end;
        }
        (start..end)
            .filter(|&b| b < self.nbits && self.get(b))
            .count()
    }

    /// `__bitmap_shift_right()`: logical right shift (`dst[i] = src[i + shift]`).
    pub fn shift_right(&mut self, src: &Bitmap, shift: usize) {
        self.assert_same_size(src);
        self.make_zero();
        if shift >= self.nbits {
            return;
        }
        for i in 0..self.nbits - shift {
            if src.get(i + shift) {
                self.set(i);
            }
        }
    }

    /// `__bitmap_shift_left()`: logical left shift (`dst[i + shift] = src[i]`).
    pub fn shift_left(&mut self, src: &Bitmap, shift: usize) {
        self.assert_same_size(src);
        self.make_zero();
        if shift >= self.nbits {
            return;
        }
        for i in 0..self.nbits - shift {
            if src.get(i) {
                self.set(i + shift);
            }
        }
    }

    /// `bitmap_cut()`: remove `[first, first + cut)` and shift the remainder
    /// left; the vacated high bits are zeroed. Requires `first + cut <= nbits`
    /// (as in C). Overlap-safe.
    pub fn cut(&mut self, first: usize, cut: usize) {
        assert!(first + cut <= self.nbits);
        let n = self.nbits;
        // dst[i] = src[i] for i < first, else src[i + cut]. Work on a clone so
        // overlapping regions cannot feed stale data back into itself (the C
        // version uses memmove plus per-bit shifting for the same reason).
        let old = self.clone();
        for i in 0..n {
            let v = if i < first {
                old.get(i)
            } else if i + cut < n {
                old.get(i + cut)
            } else {
                false
            };
            self.assign(i, v);
        }
        self.mask_tail();
    }

    /// `bitmap_read(map, start, nbits)`: read up to 64 bits starting at an
    /// arbitrary bit offset. `nbits == 0` or `> 64` yields 0 (C: undefined /
    /// no-op).
    pub fn read(&self, start: usize, nbits: usize) -> u64 {
        if nbits == 0 || nbits > BITS_PER_LONG {
            return 0;
        }
        let index = start / BITS_PER_LONG;
        let offset = start % BITS_PER_LONG;
        let space = BITS_PER_LONG - offset;
        if space >= nbits {
            return (self.words[index] >> offset) & last_word_mask(nbits);
        }
        let value_low = self.words[index] & first_word_mask(start);
        let value_high = self.words[index + 1] & last_word_mask(start + nbits);
        (value_low >> offset) | (value_high << space)
    }

    /// `bitmap_write(map, value, start, nbits)`: write `nbits` low bits of
    /// `value` at an arbitrary bit offset; bits beyond `nbits` of `value` are
    /// ignored, exactly like the C implementation.
    ///
    /// Deviation from C: grows storage if `start + nbits` exceeds the current
    /// allocation instead of relying on the caller's buffer (the declared
    /// `nbits` of the bitmap itself is unchanged).
    pub fn write(&mut self, value: u64, start: usize, nbits: usize) {
        if nbits == 0 || nbits > BITS_PER_LONG {
            return;
        }
        let mask = last_word_mask(nbits);
        let value = value & mask;
        let offset = start % BITS_PER_LONG;
        let space = BITS_PER_LONG - offset;
        let fit = space >= nbits;
        let index = start / BITS_PER_LONG;

        if index + if fit { 1 } else { 2 } > self.words.len() {
            self.words.resize(index + if fit { 1 } else { 2 }, 0);
        }

        self.words[index] &= if fit {
            !(mask << offset)
        } else {
            !first_word_mask(start)
        };
        self.words[index] |= value << offset;
        if fit {
            return;
        }
        self.words[index + 1] &= first_word_mask(start + nbits);
        self.words[index + 1] |= value >> space;
    }

    /// `bitmap_get_value8(map, start)`.
    pub fn get_value8(&self, start: usize) -> u64 {
        self.read(start, 8)
    }

    /// `bitmap_set_value8(map, value, start)`.
    pub fn set_value8(&mut self, value: u64, start: usize) {
        self.write(value, start, 8);
    }

    /// Iterate positions of set bits, ascending.
    pub fn iter_set_bits(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.nbits).filter(|&b| self.get(b))
    }

    /// Iterate positions of cleared bits, ascending.
    pub fn iter_clear_bits(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.nbits).filter(|&b| !self.get(b))
    }
}

// ---------------------------------------------------------------------------
// find_bit family over raw word slices (mirrors lib/find_bit.c).
//
// All functions take the logical `size` in bits and treat bits at
// `pos >= size` as clear regardless of the underlying words. Callers must
// provide at least `bits_to_longs(size)` words. Not found => `size`.
// ---------------------------------------------------------------------------

/// Validate that `addr` holds enough words for `size` bits (C callers pass
/// pointers whose backing storage must already satisfy this).
fn check_len(addr: &[u64], size: usize) {
    assert!(
        addr.len() >= bits_to_longs(size),
        "word slice too short: {} words for {} bits",
        addr.len(),
        size
    );
}

/// `_find_first_bit()`.
pub fn find_first_bit(addr: &[u64], size: usize) -> usize {
    find_first_generic(addr, size, |a| a)
}

/// `_find_first_zero_bit()`.
pub fn find_first_zero_bit(addr: &[u64], size: usize) -> usize {
    find_first_generic(addr, size, |a| !a)
}

/// `_find_first_and_bit()`.
pub fn find_first_and_bit(addr1: &[u64], addr2: &[u64], size: usize) -> usize {
    find_first_generic2(addr1, addr2, size, |a, b| a & b)
}

/// `_find_first_andnot_bit()`: first bit set in `addr1` and clear in `addr2`.
pub fn find_first_andnot_bit(addr1: &[u64], addr2: &[u64], size: usize) -> usize {
    find_first_generic2(addr1, addr2, size, |a, b| a & !b)
}

/// `_find_first_and_and_bit()`: first bit set in all three regions.
pub fn find_first_and_and_bit(addr1: &[u64], addr2: &[u64], addr3: &[u64], size: usize) -> usize {
    check_len(addr1, size);
    check_len(addr2, size);
    check_len(addr3, size);
    for idx in 0..bits_to_longs(size) {
        let val = addr1[idx] & addr2[idx] & addr3[idx];
        if val != 0 {
            return (idx * BITS_PER_LONG + ffs_nonzero(val)).min(size);
        }
    }
    size
}

fn find_first_generic(addr: &[u64], size: usize, munge: impl Fn(u64) -> u64) -> usize {
    check_len(addr, size);
    for (idx, &word) in addr.iter().enumerate().take(bits_to_longs(size)) {
        let val = munge(word);
        if val != 0 {
            return (idx * BITS_PER_LONG + ffs_nonzero(val)).min(size);
        }
    }
    size
}

fn find_first_generic2(
    addr1: &[u64],
    addr2: &[u64],
    size: usize,
    combine: impl Fn(u64, u64) -> u64,
) -> usize {
    check_len(addr1, size);
    check_len(addr2, size);
    for idx in 0..bits_to_longs(size) {
        let val = combine(addr1[idx], addr2[idx]);
        if val != 0 {
            return (idx * BITS_PER_LONG + ffs_nonzero(val)).min(size);
        }
    }
    size
}

/// Shared worker mirroring `FIND_NEXT_BIT(FETCH, MUNGE, size, start)`.
fn find_next_generic(fetch: impl Fn(usize) -> u64, size: usize, start: usize) -> usize {
    if start >= size {
        return size;
    }
    let mask = first_word_mask(start);
    let mut idx = start / BITS_PER_LONG;
    let mut tmp = fetch(idx) & mask;
    while tmp == 0 {
        if (idx + 1) * BITS_PER_LONG >= size {
            return size;
        }
        idx += 1;
        tmp = fetch(idx);
    }
    (idx * BITS_PER_LONG + ffs_nonzero(tmp)).min(size)
}

/// `_find_next_bit()`.
pub fn find_next_bit(addr: &[u64], size: usize, start: usize) -> usize {
    check_len(addr, size);
    find_next_generic(|idx| addr[idx], size, start)
}

/// `_find_next_zero_bit()`.
pub fn find_next_zero_bit(addr: &[u64], size: usize, start: usize) -> usize {
    check_len(addr, size);
    find_next_generic(|idx| !addr[idx], size, start)
}

/// `_find_next_and_bit()`.
pub fn find_next_and_bit(addr1: &[u64], addr2: &[u64], size: usize, start: usize) -> usize {
    check_len(addr1, size);
    check_len(addr2, size);
    find_next_generic(|idx| addr1[idx] & addr2[idx], size, start)
}

/// `_find_next_andnot_bit()`.
pub fn find_next_andnot_bit(addr1: &[u64], addr2: &[u64], size: usize, start: usize) -> usize {
    check_len(addr1, size);
    check_len(addr2, size);
    find_next_generic(|idx| addr1[idx] & !addr2[idx], size, start)
}

/// `_find_next_or_bit()`.
pub fn find_next_or_bit(addr1: &[u64], addr2: &[u64], size: usize, start: usize) -> usize {
    check_len(addr1, size);
    check_len(addr2, size);
    find_next_generic(|idx| addr1[idx] | addr2[idx], size, start)
}

/// Shared worker mirroring `FIND_NTH_BIT(FETCH, size, num)`.
fn find_nth_generic(fetch: impl Fn(usize) -> u64, size: usize, n: usize) -> usize {
    let words = bits_to_longs(size);
    let mut remaining = n;
    for idx in 0..words {
        let full_words = size / BITS_PER_LONG;
        let tmp = if idx < full_words {
            fetch(idx)
        } else {
            fetch(idx) & last_word_mask(size)
        };
        let w = tmp.count_ones() as usize;
        if w > remaining {
            return idx * BITS_PER_LONG + fns(tmp, remaining);
        }
        remaining -= w;
    }
    size
}

/// `__find_nth_bit()`: position of the `n`-th (0-indexed) set bit, or `size`.
pub fn find_nth_bit(addr: &[u64], size: usize, n: usize) -> usize {
    check_len(addr, size);
    find_nth_generic(|idx| addr[idx], size, n)
}

/// `__find_nth_and_bit()`.
pub fn find_nth_and_bit(addr1: &[u64], addr2: &[u64], size: usize, n: usize) -> usize {
    check_len(addr1, size);
    check_len(addr2, size);
    find_nth_generic(|idx| addr1[idx] & addr2[idx], size, n)
}

/// `__find_nth_and_andnot_bit()`.
pub fn find_nth_and_andnot_bit(
    addr1: &[u64],
    addr2: &[u64],
    addr3: &[u64],
    size: usize,
    n: usize,
) -> usize {
    check_len(addr1, size);
    check_len(addr2, size);
    check_len(addr3, size);
    find_nth_generic(|idx| addr1[idx] & addr2[idx] & !addr3[idx], size, n)
}

/// `_find_last_bit()`.
pub fn find_last_bit(addr: &[u64], size: usize) -> usize {
    check_len(addr, size);
    if size == 0 {
        return 0;
    }
    let mut val = last_word_mask(size);
    let mut idx = (size - 1) / BITS_PER_LONG;
    loop {
        val &= addr[idx];
        if val != 0 {
            return idx * BITS_PER_LONG + (BITS_PER_LONG - 1 - val.leading_zeros() as usize);
        }
        val = !0u64;
        if idx == 0 {
            break;
        }
        idx -= 1;
    }
    size
}

/// `find_next_clump8()`: next multiple-of-8-aligned clump containing a set
/// bit at or after `offset`; writes the 8-bit clump value to `*clump`.
pub fn find_next_clump8(clump: &mut u64, addr: &[u64], size: usize, offset: usize) -> usize {
    let offset = find_next_bit(addr, size, offset);
    if offset == size {
        return size;
    }
    let offset = offset & !7usize;
    *clump = get_value8_slice(addr, offset);
    offset
}

/// `bitmap_get_value8()` over a raw slice.
pub fn get_value8_slice(map: &[u64], start: usize) -> u64 {
    read_slice(map, start, 8)
}

/// `bitmap_write()`'s `bitmap_set_value8()` over a raw slice.
pub fn set_value8_slice(map: &mut [u64], value: u64, start: usize) {
    write_slice(map, value, start, 8)
}

/// `bitmap_read()` over a raw slice (requires capacity, like C).
pub fn read_slice(map: &[u64], start: usize, nbits: usize) -> u64 {
    if nbits == 0 || nbits > BITS_PER_LONG {
        return 0;
    }
    let index = start / BITS_PER_LONG;
    let offset = start % BITS_PER_LONG;
    let space = BITS_PER_LONG - offset;
    if space >= nbits {
        return (map[index] >> offset) & last_word_mask(nbits);
    }
    let value_low = map[index] & first_word_mask(start);
    let value_high = map[index + 1] & last_word_mask(start + nbits);
    (value_low >> offset) | (value_high << space)
}

/// `bitmap_write()` over a raw slice (requires capacity, like C).
pub fn write_slice(map: &mut [u64], value: u64, start: usize, nbits: usize) {
    if nbits == 0 || nbits > BITS_PER_LONG {
        return;
    }
    let mask = last_word_mask(nbits);
    let value = value & mask;
    let offset = start % BITS_PER_LONG;
    let space = BITS_PER_LONG - offset;
    let fit = space >= nbits;
    let index = start / BITS_PER_LONG;

    map[index] &= if fit {
        !(mask << offset)
    } else {
        !first_word_mask(start)
    };
    map[index] |= value << offset;
    if fit {
        return;
    }
    map[index + 1] &= first_word_mask(start + nbits);
    map[index + 1] |= value >> space;
}

/// `bitmap_find_next_zero_area_off()`: find an aligned run of `nr` clear bits.
///
/// Returns the bit offset of the area, or a value `>= size` if none exists.
pub fn find_next_zero_area_off(
    map: &[u64],
    size: usize,
    mut start: usize,
    nr: usize,
    align_mask: usize,
    align_offset: usize,
) -> usize {
    loop {
        start = find_next_zero_bit(map, size, start);
        if start >= size {
            return size;
        }
        start = ((start + align_offset) | align_mask) - align_offset;
        let end = start + nr;
        if end > size {
            return size;
        }

        let off = start & !(BITS_PER_LONG - 1);
        // Search window [off, end): C passes map + start/BITS_PER_LONG with
        // length end - off, i.e. relative to the containing word boundary.
        let sub = &map[off / BITS_PER_LONG..];
        let i = find_last_bit(sub, end - off) + off;
        if i >= end || i < start {
            return start;
        }
        start = i;
    }
}

#[cfg(test)]
mod tests;
