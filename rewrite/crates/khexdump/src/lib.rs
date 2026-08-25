// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's `lib/hexdump.c`.
//!
//! C-to-Rust correspondence:
//!
//! | C (lib/hexdump.c)                          | Rust (this crate)                            |
//! |--------------------------------------------|----------------------------------------------|
//! | `hex_asc` / `hex_asc_upper`                | [`HEX_ASC`] / [`HEX_ASC_UPPER`]              |
//! | `hex_to_bin(ch)`                           | [`hex_to_bin`] (same branchless arithmetic)  |
//! | `hex2bin(dst, src, count)`                 | [`hex2bin`]                                  |
//! | `bin2hex(dst, src, count)`                 | [`bin2hex`]                                  |
//! | `hex_dump_to_buffer(...)`                  | [`hex_dump_to_buffer`]                       |
//! | internal `linebuf[32*3+2+32+1]` in print_hex_dump | [`PRINT_LINEBUF_SIZE`], [`HexLine`]   |
//! | `print_hex_dump(..., DUMP_PREFIX_OFFSET|NONE)` | [`print_hex_dump_lines`]                 |
//!
//! Faithfulness notes / deviations from C:
//! - Slices carry lengths; the separate `len` parameters disappear. NULL
//!   pointers are unrepresentable.
//! - `hex2bin`: C reads past the end of `src` when `src` is shorter than
//!   `2 * count` (unchecked); that is memory-unsafe UB, so this rewrite
//!   returns [`HexError::TooShort`] instead. Bad digits remain
//!   [`HexError::Invalid`] (`-EINVAL`), matching C.
//! - `bin2hex`: same treatment — C requires the caller to provide
//!   `2 * count` bytes of `dst` and silently corrupts memory otherwise;
//!   here a short `dst` yields [`HexError::TooShort`].
//! - Group reads use native-endian byte loads (`from_ne_bytes`), which is
//!   exactly what `get_unaligned()` does on each architecture.
//! - `rowsize`/`groupsize`/`len` are `usize`, so negative values (which C
//!   normalizes to 16/1 respectively) cannot occur.
//! - `hex_dump_to_buffer` returns `isize` to reproduce one observable C
//!   quirk: with `linebuflen == 0` and empty input and `ascii == false`,
//!   the overflow formula `(groupsize * 2 + 1) * ngroups - 1` evaluates
//!   to `-1` in C `int` arithmetic, and that value escapes to callers.
//! - `print_hex_dump` printed via `printk`; there is no syslog here, so it
//!   becomes an iterator of formatted lines ([`print_hex_dump_lines`]).
//!   Consumers reproduce `DUMP_PREFIX_OFFSET` by prepending
//!   [`DumpLine::offset_prefix_bytes`] and `DUMP_PREFIX_NONE` by prepending
//!   nothing. `DUMP_PREFIX_ADDRESS` is omitted entirely: printing raw
//!   kernel virtual addresses has no meaning outside the kernel and would
//!   leak KASLR state.

#![no_std]
#![deny(unsafe_code)]

/// `hex_asc`: lowercase hex digit lookup table.
pub const HEX_ASC: &[u8; 16] = b"0123456789abcdef";
/// `hex_asc_upper`: uppercase hex digit lookup table.
pub const HEX_ASC_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// Size of the line buffer `print_hex_dump()` puts on the stack in C:
/// `32 * 3 + 2 + 32 + 1`. Any single line produced for `rowsize <= 32`
/// fits without truncation.
pub const PRINT_LINEBUF_SIZE: usize = 32 * 3 + 2 + 32 + 1;

/// Error type mirroring the negative-errno returns of the C hex helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexError {
    /// Invalid input character (C: `-EINVAL`).
    Invalid,
    /// Input/output buffer too short (C: silent out-of-bounds access).
    TooShort,
}

impl HexError {
    /// Negative errno matching the C return value where one exists.
    pub const fn errno(self) -> i32 {
        match self {
            HexError::Invalid => -22,  // -EINVAL
            HexError::TooShort => -90, // -EMSGSIZE (no C counterpart)
        }
    }
}

/// `hex_to_bin()`: convert one ASCII hex digit to its value, or -1.
///
/// This keeps the C implementation's branch-free arithmetic (the kernel
/// codes it this way so cryptographic key loading has no data-dependent
/// branches). A `match` would have equivalent observable behavior but
/// reintroduce data-dependent control flow.
#[inline]
pub fn hex_to_bin(ch: u8) -> i32 {
    let c = i32::from(ch);
    // (ch & 0xdf): lowercase -> uppercase.
    let cu = c & 0xdf;

    // Mask is non-zero (with high bits set after >> 8) iff '0' <= ch <= '9'.
    let digit_mask =
        ((c.wrapping_sub(b'9' as i32).wrapping_sub(1)) & (b'0' as i32 - 1 - c)) as u32 >> 8;
    let letter_mask =
        ((cu.wrapping_sub(b'F' as i32).wrapping_sub(1)) & (b'A' as i32 - 1 - cu)) as u32 >> 8;

    -1 + ((c.wrapping_sub(b'0' as i32).wrapping_add(1)) & digit_mask as i32)
        + ((cu.wrapping_sub(b'A' as i32).wrapping_add(11)) & letter_mask as i32)
}

/// `hex_byte_pack()`: write one byte as two lowercase hex digits at
/// `dst[pos..]`; returns the new position. Caller must ensure room for 2.
#[inline]
fn hex_byte_pack(dst: &mut [u8], pos: usize, byte: u8) -> usize {
    dst[pos] = HEX_ASC[(byte >> 4) as usize];
    dst[pos + 1] = HEX_ASC[(byte & 0xf) as usize];
    pos + 2
}

/// `hex2bin()`: convert an ASCII hex string to binary.
///
/// Converts `dst.len()` bytes, reading two hex digits per output byte from
/// `src`. Returns [`HexError::Invalid`] on any non-hex character (matching
/// C's `-EINVAL`) and [`HexError::TooShort`] when `src` holds fewer than
/// `2 * dst.len()` digits (C read past the buffer there; see module docs).
pub fn hex2bin(dst: &mut [u8], src: &[u8]) -> Result<(), HexError> {
    if src.len() < dst.len() * 2 {
        return Err(HexError::TooShort);
    }
    for (i, o) in dst.iter_mut().enumerate() {
        let hi = hex_to_bin(src[2 * i]);
        if hi < 0 {
            return Err(HexError::Invalid);
        }
        let lo = hex_to_bin(src[2 * i + 1]);
        if lo < 0 {
            return Err(HexError::Invalid);
        }
        *o = ((hi << 4) | lo) as u8;
    }
    Ok(())
}

/// `bin2hex()`: convert binary data to lowercase ASCII hex.
///
/// On success returns the subslice of `dst` just past the written digits,
/// mirroring the returned pointer of the C function. Requires
/// `dst.len() >= 2 * src.len()`; otherwise [`HexError::TooShort`] (the C
/// version relied on the caller and corrupted memory otherwise).
pub fn bin2hex<'a>(dst: &'a mut [u8], src: &[u8]) -> Result<&'a mut [u8], HexError> {
    if dst.len() < src.len() * 2 {
        return Err(HexError::TooShort);
    }
    let mut pos = 0;
    for &b in src {
        pos = hex_byte_pack(dst, pos, b);
    }
    Ok(&mut dst[pos..])
}

/// Kernel's `is_power_of_2()` restricted to `usize`.
const fn is_power_of_2(x: usize) -> bool {
    x != 0 && (x & (x - 1)) == 0
}

/// Emulate `snprintf(linebuf + lx, linebuflen - lx, "%s%s", sep, s)` for the
/// bounded group writes, including snprintf's truncated-write side effects.
///
/// Returns `false` when the caller must take the C `overflow1` path. On
/// truncation this still writes the fitting prefix plus NUL exactly like
/// `snprintf` does, because the kernel self-test compares full buffer bytes.
fn append_bounded(linebuf: &mut [u8], lx: &mut usize, sep: Option<u8>, s: &[u8]) -> bool {
    let total = usize::from(sep.is_some()) + s.len();
    let rem = linebuf.len() - *lx;
    if total >= rem {
        // snprintf(size=rem) writes rem-1 chars then NUL (nothing when 0).
        if rem > 0 {
            let mut p = *lx;
            if let Some(c) = sep {
                if p < *lx + rem - 1 {
                    linebuf[p] = c;
                    p += 1;
                }
            }
            let body = *lx + rem - 1 - p;
            linebuf[p..p + body].copy_from_slice(&s[..body]);
            *lx += rem - 1;
            linebuf[*lx] = 0;
        }
        return false;
    }
    if let Some(c) = sep {
        linebuf[*lx] = c;
        *lx += 1;
    }
    linebuf[*lx..*lx + s.len()].copy_from_slice(s);
    *lx += s.len();
    true
}

/// `hex_dump_to_buffer()`: convert one "line" of data to hex (+ASCII) text.
///
/// Works on one row at a time: at most `rowsize` bytes of `buf` are
/// converted (C caps `len` internally the same way). The converted output
/// is always NUL-terminated inside `linebuf` when `linebuf` is non-empty.
///
/// Normalization identical to C:
/// - `rowsize` not 16 or 32 becomes 16;
/// - `len` is capped to `rowsize`;
/// - `groupsize` not a power of two or greater than 8 becomes 1;
/// - a `groupsize` that does not divide `len` becomes 1 ("no mixed size
///   output").
///
/// Return semantics identical to C, including truncation: normally the
/// number of bytes placed in the buffer without the terminating NUL; if
/// truncated, the number of bytes that *would* have been written had there
/// been room (excluding the NUL). The lone negative case (`isize`) is
/// documented in the module docs.
pub fn hex_dump_to_buffer(
    buf: &[u8],
    rowsize: usize,
    groupsize: usize,
    linebuf: &mut [u8],
    ascii: bool,
) -> isize {
    let rowsize = if rowsize != 16 && rowsize != 32 {
        16
    } else {
        rowsize
    };
    let len = buf.len().min(rowsize);
    let groupsize = if !is_power_of_2(groupsize) || groupsize > 8 || len % groupsize != 0 {
        1
    } else {
        groupsize
    };

    let ngroups = len / groupsize;
    let ascii_column = rowsize * 2 + rowsize / groupsize + 1;

    if linebuf.is_empty() {
        return overflow1_ret(ascii, ascii_column, len, groupsize, ngroups);
    }

    if len == 0 {
        linebuf[0] = 0;
        return 0;
    }

    let mut lx: usize = 0;

    if groupsize == 8 || groupsize == 4 || groupsize == 2 {
        let width = groupsize * 2;
        for j in 0..ngroups {
            let off = j * groupsize;
            let mut val = [0u8; 8];
            val[..groupsize].copy_from_slice(&buf[off..off + groupsize]);
            let v = match groupsize {
                8 => u64::from_ne_bytes(val),
                4 => u32::from_ne_bytes([val[0], val[1], val[2], val[3]]) as u64,
                _ => u16::from_ne_bytes([val[0], val[1]]) as u64,
            };
            let mut digits = [0u8; 16];
            let mut v = v;
            for k in (0..width).rev() {
                digits[k] = HEX_ASC[(v & 0xf) as usize];
                v >>= 4;
            }
            if !append_bounded(
                linebuf,
                &mut lx,
                if j != 0 { Some(b' ') } else { None },
                &digits[..width],
            ) {
                return overflow1_ret(ascii, ascii_column, len, groupsize, ngroups);
            }
        }
    } else {
        for &ch in &buf[..len] {
            // C guards each individual character write (three per byte:
            // hi digit, lo digit, space). Fusing the two digits into one
            // unguarded 16-bit pack would overshoot on tiny buffers.
            if linebuf.len() < lx + 2 {
                return overflow2_ret(linebuf, lx, ascii, ascii_column, len, groupsize, ngroups);
            }
            linebuf[lx] = HEX_ASC[(ch >> 4) as usize];
            lx += 1;
            if linebuf.len() < lx + 2 {
                return overflow2_ret(linebuf, lx, ascii, ascii_column, len, groupsize, ngroups);
            }
            linebuf[lx] = HEX_ASC[(ch & 0xf) as usize];
            lx += 1;
            if linebuf.len() < lx + 2 {
                return overflow2_ret(linebuf, lx, ascii, ascii_column, len, groupsize, ngroups);
            }
            linebuf[lx] = b' ';
            lx += 1;
        }
        lx -= 1; // drop trailing space (j > 0 guaranteed: len != 0)
    }

    if !ascii {
        linebuf[lx] = 0;
        return lx as isize;
    }

    while lx < ascii_column {
        if linebuf.len() < lx + 2 {
            return overflow2_ret(linebuf, lx, ascii, ascii_column, len, groupsize, ngroups);
        }
        linebuf[lx] = b' ';
        lx += 1;
    }
    for &ch in &buf[..len] {
        if linebuf.len() < lx + 2 {
            return overflow2_ret(linebuf, lx, ascii, ascii_column, len, groupsize, ngroups);
        }
        // C: isascii(ch) && isprint(ch) ? ch : '.'
        linebuf[lx] = if (0x20..=0x7e).contains(&ch) {
            ch
        } else {
            b'.'
        };
        lx += 1;
    }

    linebuf[lx] = 0;
    lx as isize
}

fn overflow1_ret(ascii: bool, ascii_column: usize, len: usize, gs: usize, ngroups: usize) -> isize {
    if ascii {
        (ascii_column + len) as isize
    } else {
        ((gs * 2 + 1).wrapping_mul(ngroups)).wrapping_sub(1) as isize
    }
}

/// C `overflow2`: terminate what fits at `lx`, then return the C
/// `overflow1` formula. Every guard that jumps here runs before a write and
/// only advances `lx` when room existed for char + NUL, so `lx <
/// linebuf.len()` always holds and the NUL write is in-bounds (in C too).
fn overflow2_ret(
    linebuf: &mut [u8],
    lx: usize,
    ascii: bool,
    ascii_column: usize,
    len: usize,
    gs: usize,
    ngroups: usize,
) -> isize {
    linebuf[lx] = 0;
    overflow1_ret(ascii, ascii_column, len, gs, ngroups)
}

/// A fully formatted single dump line plus its NUL-free length.
///
/// Mirrors the `char linebuf[32 * 3 + 2 + 32 + 1]` that C's
/// `print_hex_dump()` uses internally: large enough that no valid line ever
/// truncates.
#[derive(Clone, Copy)]
pub struct HexLine {
    buf: [u8; PRINT_LINEBUF_SIZE],
    len: usize,
}

impl HexLine {
    fn new() -> Self {
        HexLine {
            buf: [0; PRINT_LINEBUF_SIZE],
            len: 0,
        }
    }

    /// Formatted contents (no NUL).
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Formatted contents as ASCII `str`.
    pub fn as_str(&self) -> &str {
        // Every byte emitted by hex_dump_to_buffer is ASCII by construction.
        core::str::from_utf8(self.as_bytes()).unwrap_or("")
    }
}

impl core::fmt::Debug for HexLine {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Format one line into a [`HexLine`] (never truncates for
/// `rowsize <= 32`). Equivalent to C calling `hex_dump_to_buffer()` with
/// its full-size stack buffer.
pub fn hex_dump_line(buf: &[u8], rowsize: usize, groupsize: usize, ascii: bool) -> HexLine {
    let mut line = HexLine::new();
    let n = hex_dump_to_buffer(buf, rowsize, groupsize, &mut line.buf, ascii);
    line.len = n.max(0) as usize;
    line
}

/// One yielded line of [`print_hex_dump_lines`].
#[derive(Clone, Copy)]
pub struct DumpLine {
    /// Byte offset of this row within the dumped buffer.
    pub offset: usize,
    /// Formatted row (hex [+ ASCII]).
    pub line: HexLine,
}

impl DumpLine {
    /// `"xxxxxxxx: "` offset prefix bytes (`%.8x: `), lowercase.
    pub fn offset_prefix_bytes(&self) -> [u8; 10] {
        let mut out = [b' '; 10];
        for i in 0..8 {
            out[i] = HEX_ASC[(self.offset >> (4 * (7 - i))) & 0xf];
        }
        out[8] = b':';
        out[9] = b' ';
        out
    }
}

/// Iterator over formatted dump lines; the printk-free replacement for C
/// `print_hex_dump()` (see module docs for the ADDRESS-prefix omission).
pub struct DumpLines<'a> {
    rest: &'a [u8],
    pos: usize,
    rowsize: usize,
    groupsize: usize,
    ascii: bool,
}

/// Iterate the dump lines of `buf`, chunked by `rowsize`, mirroring the
/// iteration structure of C `print_hex_dump()`.
pub fn print_hex_dump_lines(
    buf: &[u8],
    rowsize: usize,
    groupsize: usize,
    ascii: bool,
) -> DumpLines<'_> {
    let rowsize = if rowsize != 16 && rowsize != 32 {
        16
    } else {
        rowsize
    };
    DumpLines {
        rest: buf,
        pos: 0,
        rowsize,
        groupsize,
        ascii,
    }
}

impl<'a> Iterator for DumpLines<'a> {
    type Item = DumpLine;

    fn next(&mut self) -> Option<DumpLine> {
        if self.rest.is_empty() {
            return None;
        }
        let linelen = self.rest.len().min(self.rowsize);
        let (chunk, tail) = self.rest.split_at(linelen);
        self.rest = tail;
        let line = hex_dump_line(chunk, self.rowsize, self.groupsize, self.ascii);
        let item = DumpLine {
            offset: self.pos,
            line,
        };
        self.pos += linelen;
        Some(item)
    }
}

#[cfg(test)]
mod tests;
