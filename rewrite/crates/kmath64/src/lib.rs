// SPDX-License-Identifier: GPL-2.0
//! Rust rewrite of the Linux kernel's 64-bit arithmetic helpers:
//! `lib/math/div64.c`, `lib/math/gcd.c` and `lib/math/reciprocal_div.c`,
//! plus the small inline helpers from `include/linux/math64.h`,
//! `include/vdso/math64.h` and `include/linux/reciprocal_div.h` that complete
//! those units.
//!
//! # C-to-Rust correspondence
//!
//! | C symbol (source)                              | Rust function                       |
//! |------------------------------------------------|-------------------------------------|
//! | `div_u64_rem` (math64.h)                       | [`div_u64_rem`] / [`checked_div_u64_rem`] |
//! | `div_u64` (math64.h)                           | [`div_u64`]                          |
//! | `__div64_32` (div64.c, 32-bit emulation)       | [`div64_32`]                         |
//! | `div_s64_rem` (div64.c 32-bit path / math64.h) | [`div_s64_rem`]                      |
//! | `div_s64` (math64.h)                           | [`div_s64`]                          |
//! | `div64_u64_rem` (div64.c, Hacker's Delight)    | [`div64_u64_rem`]                    |
//! | `div64_u64` (div64.c, Hacker's Delight)        | [`div64_u64`]                        |
//! | `div64_s64` (div64.c)                          | [`div64_s64`]                        |
//! | `iter_div_u64_rem` / `__iter_div_u64_rem`      | [`iter_div_u64_rem`]                 |
//! | `mul_u64_add_u64_div_u64` (div64.c)            | [`mul_u64_add_u64_div_u64`]          |
//! | `mul_u64_u64_div_u64` (math64.h macro)         | [`mul_u64_u64_div_u64`]              |
//! | `mul_u64_u64_div_u64_roundup` (math64.h macro) | [`mul_u64_u64_div_u64_roundup`]      |
//! | `mul_u64_u32_add_u64_shr` (vdso/math64.h)      | [`mul_u64_u32_add_u64_shr`]          |
//! | `gcd` (gcd.c, binary GCD w/ ffs fast path)     | [`gcd`]                              |
//! | gcd.c even/odd fallback branch                 | [`gcd_even_odd`]                     |
//! | `fls` (asm-generic/bitops/fls.h)               | [`fls32`]                            |
//! | `__ffs` (asm-generic/bitops)                   | [`ffs64`]                            |
//! | `reciprocal_value` (reciprocal_div.c)          | [`reciprocal_value`]                 |
//! | `reciprocal_divide` (reciprocal_div.h, inline) | [`reciprocal_divide`]                |
//! | `reciprocal_value_adv` (reciprocal_div.c)      | [`reciprocal_value_adv`]             |
//! | usage pseudo-code (reciprocal_div.h comment)   | [`divide_with_reciprocal_adv`]       |
//!
//! # Division-by-zero and overflow contracts (C UB made explicit)
//!
//! The C code is *undefined* in several corners; this crate makes each one a
//! documented, deterministic choice instead:
//!
//! * **Zero divisor**: C traps (CPU #DE exception). Every division function
//!   here **panics** on a zero divisor — except [`iter_div_u64_rem`], whose C
//!   implementation would *hang forever* on a zero divisor rather than trap
//!   (it is a pure subtraction loop); we panic instead of hanging.
//! * **`i64::MIN / -1`** (and `i64::MIN % -1`): UB in C, faulting on x86.
//!   The signed dividers here **panic**. Note `|i64::MIN| == 2^63` is
//!   representable as a `u64` magnitude, so `i64::MIN / d` for any other `d`
//!   works and `i64::MIN / 1 == i64::MIN`.
//! * **`abs(i64::MIN)` / `abs(i32::MIN)`** inside the C implementations is
//!   itself UB; these ports compute magnitudes in the unsigned domain and
//!   never negate `MIN`.
//! * **[`mul_u64_add_u64_div_u64`] overflow** (`a*b + c >= 2^64`, quotient
//!   unrepresentable): C *defines* this case to return `~0ULL`; we return
//!   [`u64::MAX`] identically (see [`MUL_OVERFLOW`]).
//! * **[`iter_div_u64_rem`] quotient past `u32::MAX`**: C silently wraps its
//!   `u32` counter; we reproduce that with a wrapping increment so behavior
//!   is bit-identical (the contract is "dividend not much bigger than
//!   divisor").
//! * **[`reciprocal_value_adv`] with `d >= 2^31`**: the C code `WARN`s and
//!   then performs an invalid `1ULL << 64` shift (garbage). The header
//!   declares such calls unsupported, so we return [`Option::None`].
//! * **`reciprocal_value_adv(d, prec)` with `prec > 32 + ceil(log2(d))`**:
//!   invalid shift in C (UB); we `panic!` in debug builds and mirror C's
//!   wrapping-shift behavior in release.
//!
//! Where C compiles these helpers differently per architecture (64-bit builds
//! get native `/`/`%` inline wrappers from `math64.h`; 32-bit builds get the
//! shift-subtract algorithms from `div64.c`), this crate provides the
//! algorithmic implementations and differential-tests them against native
//! arithmetic as the oracle: wherever C defines a result, the algorithms and
//! native division must agree exactly.

#![no_std]
#![deny(unsafe_code)]

/// Value returned by [`mul_u64_add_u64_div_u64`] when the true quotient does
/// not fit in a `u64` (C: `~0ULL`). Distinct from the divide-by-zero trap.
pub const MUL_OVERFLOW: u64 = u64::MAX;

/// Kernel `fls(x)` for `unsigned int`: 1-based index of the most significant
/// set bit; `fls(0) == 0`. (`asm-generic/bitops/fls.h`)
#[inline]
#[must_use]
pub const fn fls32(x: u32) -> u32 {
    if x == 0 { 0 } else { 32 - x.leading_zeros() }
}

/// Kernel `__ffs(x)` for `unsigned long`: 0-based index of the least
/// significant set bit. Precondition `x != 0`, same as C (which returns
/// garbage for 0).
#[inline]
#[must_use]
pub const fn ffs64(x: u64) -> u32 {
    x.trailing_zeros()
}

// ---------------------------------------------------------------------------
// Unsigned 64/32 division
// ---------------------------------------------------------------------------

/// `div_u64_rem()`: unsigned 64/32 division with remainder.
///
/// Panics when `divisor == 0` (C: trap).
///
/// Returns `(quotient, remainder)` where the C version returns the quotient
/// and writes `*remainder`.
#[inline]
#[must_use]
pub fn div_u64_rem(dividend: u64, divisor: u32) -> (u64, u32) {
    assert!(divisor != 0, "div_u64_rem: division by zero");
    (
        dividend / u64::from(divisor),
        (dividend % u64::from(divisor)) as u32,
    )
}

/// Total variant of [`div_u64_rem`]: `None` iff `divisor == 0`.
#[inline]
#[must_use]
pub const fn checked_div_u64_rem(dividend: u64, divisor: u32) -> Option<(u64, u32)> {
    if divisor == 0 {
        None
    } else {
        let d = divisor as u64;
        Some((dividend / d, (dividend % d) as u32))
    }
}

/// `div_u64()`: unsigned 64/32 division. Panics on zero divisor (C: trap).
#[inline]
#[must_use]
pub fn div_u64(dividend: u64, divisor: u32) -> u64 {
    let (q, _) = div_u64_rem(dividend, divisor);
    q
}

/// `__div64_32()`: generic shift-subtract 64-by-32 division from
/// `lib/math/div64.c` (used by 32-bit architectures without hardware divide).
///
/// The C signature mutates `*n` into the quotient and returns the remainder;
/// this version takes the dividend by value and returns
/// `(quotient, remainder)`. Panics on zero divisor (in C the zero eventually
/// reaches a hardware divide).
///
/// Deliberately the *same algorithm* as C (doubling trick + restore loop),
/// not a call to native division, so the differential tests exercise the
/// algorithm itself.
#[must_use]
pub fn div64_32(dividend: u64, base: u32) -> (u64, u32) {
    assert!(base != 0, "__div64_32: division by zero");
    let mut rem = dividend;
    let b_orig = u64::from(base);
    let mut res: u64 = 0;
    let mut d: u64 = 1;
    let mut high = rem >> 32;

    // Reduce the thing a bit first.
    if high >= b_orig {
        high /= b_orig;
        res = high << 32;
        rem -= (high * b_orig) << 32;
    }

    // Double b and d until b overflows into the sign bit (C guards with
    // `(int64_t)b > 0`, which stops doubling at 1 << 62) or until b >= rem.
    let mut b = b_orig;
    while (b as i64) > 0 && b < rem {
        b += b;
        d += d;
    }

    loop {
        if rem >= b {
            rem -= b;
            res += d;
        }
        b >>= 1;
        d >>= 1;
        if d == 0 {
            break;
        }
    }

    (res, rem as u32)
}

// ---------------------------------------------------------------------------
// Signed 64/32 division
// ---------------------------------------------------------------------------

/// `div_s64_rem()`: signed 64-by-32 division with remainder, following the C
/// magnitude-based algorithm from `lib/math/div64.c`. The remainder takes the
/// sign of the dividend (C99 truncating division), e.g.
/// `div_s64_rem(-7, 2) == (-3, -1)`.
///
/// Panics on `divisor == 0` and on `dividend == i64::MIN && divisor == -1`
/// (both are UB in C; see module docs). Magnitudes are computed in the
/// unsigned domain, so the C source's UB `abs(i64::MIN)` never occurs.
///
/// Returns `(quotient, remainder)`.
#[must_use]
pub fn div_s64_rem(dividend: i64, divisor: i32) -> (i64, i32) {
    assert!(divisor != 0, "div_s64_rem: division by zero");
    assert!(
        !(dividend == i64::MIN && divisor == -1),
        "div_s64_rem: i64::MIN / -1 overflows (UB in C)"
    );

    // Unsigned magnitudes: |i64::MIN| == 2^63 and |i32::MIN| == 2^31 fit.
    let (q_mag, r_mag) = div_u64_rem(dividend.unsigned_abs(), divisor.unsigned_abs());

    // Remainder takes the sign of the dividend (truncating semantics).
    // r_mag <= |divisor| - 1 < 2^31 always fits an i32 after re-signing.
    let remainder = if dividend < 0 {
        -(r_mag as i32)
    } else {
        r_mag as i32
    };

    // Quotient sign: negative iff operand signs differ. Computed in i128 to
    // stay total; the only magnitude that cannot be re-signed is 2^63 with a
    // negative sign, which is exactly the rejected MIN / -1 case.
    let negative = (dividend < 0) ^ (divisor < 0);
    let quotient_i128 = if negative { -(q_mag as i128) } else { q_mag as i128 };
    debug_assert!(
        (i64::MIN as i128..=i64::MAX as i128).contains(&quotient_i128),
        "unreachable: overflowing quotient escaped the MIN/-1 guard"
    );

    (quotient_i128 as i64, remainder)
}

/// `div_s64()`: signed 64-by-32 division. See [`div_s64_rem`] for contracts.
#[inline]
#[must_use]
pub fn div_s64(dividend: i64, divisor: i32) -> i64 {
    div_s64_rem(dividend, divisor).0
}

// ---------------------------------------------------------------------------
// Unsigned 64/64 division (Hacker's Delight algorithm)
// ---------------------------------------------------------------------------

/// `div64_u64()`: unsigned 64-bit division with 64-bit divisor, implementing
/// the modified Hacker's Delight algorithm from `lib/math/div64.c`
/// ('hackerdelight.org/hdcodetxt/divDouble.c.txt').
///
/// On 64-bit C builds this is an inline wrapper over native `/`; on 32-bit
/// builds it is this algorithm. We provide the algorithm and test it against
/// native division. Panics on zero divisor (C: trap).
#[must_use]
pub fn div64_u64(dividend: u64, divisor: u64) -> u64 {
    assert!(divisor != 0, "div64_u64: division by zero");
    let high = (divisor >> 32) as u32;

    if high == 0 {
        div_u64(dividend, divisor as u32)
    } else {
        let n = fls32(high);
        let mut quot = div_u64(dividend >> n, (divisor >> n) as u32);

        #[allow(clippy::implicit_saturating_sub)] // mirrors the C algorithm verbatim
        if quot != 0 {
            quot -= 1;
        }
        if dividend.wrapping_sub(quot.wrapping_mul(divisor)) >= divisor {
            quot += 1;
        }

        quot
    }
}

/// `div64_u64_rem()`: like [`div64_u64`] but also returns the remainder.
///
/// Panics on zero divisor (C: trap). Returns `(quotient, remainder)`.
#[must_use]
pub fn div64_u64_rem(dividend: u64, divisor: u64) -> (u64, u64) {
    assert!(divisor != 0, "div64_u64_rem: division by zero");
    let high = (divisor >> 32) as u32;

    if high == 0 {
        let (q, r) = div_u64_rem(dividend, divisor as u32);
        (q, u64::from(r))
    } else {
        let n = fls32(high);
        let mut quot = div_u64(dividend >> n, (divisor >> n) as u32);

        #[allow(clippy::implicit_saturating_sub)] // mirrors the C algorithm verbatim
        if quot != 0 {
            quot -= 1;
        }

        let mut rem = dividend.wrapping_sub(quot.wrapping_mul(divisor));
        if rem >= divisor {
            quot += 1;
            rem -= divisor;
        }

        (quot, rem)
    }
}

/// `div64_s64()`: signed 64-bit division with 64-bit divisor, following the C
/// sign-magnitude construction (`t = (dividend ^ divisor) >> 63`;
/// `(quot ^ t) - t`). Truncates toward zero.
///
/// Panics on `divisor == 0` and on `dividend == i64::MIN && divisor == -1`
/// (both UB in C). Magnitudes stay in the unsigned domain (`|i64::MIN|`
/// fits a `u64`), so `abs(i64::MIN)` — UB in the C source — never happens.
/// `i64::MIN / 1` correctly yields `i64::MIN` via the two's-complement
/// identity, matching the C bit trick on defined inputs.
#[must_use]
pub fn div64_s64(dividend: i64, divisor: i64) -> i64 {
    assert!(divisor != 0, "div64_s64: division by zero");
    assert!(
        !(dividend == i64::MIN && divisor == -1),
        "div64_s64: i64::MIN / -1 overflows (UB in C)"
    );

    let quot = div64_u64(dividend.unsigned_abs(), divisor.unsigned_abs());
    let signs_differ = (dividend < 0) ^ (divisor < 0);
    if signs_differ {
        // C: (quot ^ -1) - (-1) == ~quot == -quot - 1 + 1 == -quot.
        // quot < 2^63 here because MIN / -1 was rejected.
        -(quot as i128) as i64
    } else {
        // quot may be exactly 2^63 (i64::MIN / 1); the wrap cast reproduces
        // the C `(quot ^ 0) - 0` bit pattern exactly.
        quot as i64
    }
}

// ---------------------------------------------------------------------------
// Iterative division (vdso/math64.h)
// ---------------------------------------------------------------------------

/// `iter_div_u64_rem()` / `__iter_div_u64_rem()`: iterative subtract-and-count
/// division, used by timekeeping when the dividend is expected to be small
/// relative to the divisor. The loop *is* the point (bounded iteration count,
/// no hardware divide in vDSO context), so it is preserved verbatim.
///
/// Returns `(quotient, remainder)`. The C quotient counter is a `u32` and
/// silently wraps past `u32::MAX`; we reproduce that with `wrapping_add`.
///
/// **Deviation:** C *hangs forever* on a zero divisor (the loop never
/// terminates and no trap occurs); we panic instead of hanging.
#[must_use]
pub fn iter_div_u64_rem(mut dividend: u64, divisor: u32) -> (u32, u64) {
    assert!(
        divisor != 0,
        "iter_div_u64_rem: division by zero (C hangs forever here)"
    );
    let mut ret: u32 = 0;
    while dividend >= u64::from(divisor) {
        dividend -= u64::from(divisor);
        ret = ret.wrapping_add(1);
    }
    (ret, dividend)
}

// ---------------------------------------------------------------------------
// mul_u64_add_u64_div_u64 (lib/math/div64.c)
// ---------------------------------------------------------------------------

/// `mul_u64_u64_add_u64()` from div64.c, 32-bit-limb schoolbook variant:
/// computes the exact 128-bit value `a * b + c` split into `(high, low)`
/// 64-bit halves using only 32x32->64 partial products and carry propagation
/// (no 128-bit type), mirroring the `#else` branch of the C source used when
/// the compiler lacks `__int128`. Kept as the production path so the chunk
/// arithmetic is exercised by every test of [`mul_u64_add_u64_div_u64`].
///
/// The C helper macros expand to 32x32->64 multiplications with wrapping adds
/// (`mul_add(a, b, c) == (u64)(u32)a * (u32)b + c`, `add_u64_u32(a, b) ==
/// a + b`); both truncate their operands exactly as done here.
#[must_use]
pub fn mul_u64_u64_add_u64_chunks(a: u64, b: u64, c: u64) -> (u64, u64) {
    const M: u64 = 0xffff_ffff;
    let a0 = a & M;
    let a1 = a >> 32;
    let b0 = b & M;
    let b1 = b >> 32;

    let p00 = a0 * b0; // < 2^64
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;

    // Sum low 32 bits first: p00 + (c & M) cannot overflow (< 2^64).
    let s = p00 + (c & M);
    let l0 = s & M;
    let k0 = s >> 32; // < 2^32

    // Middle column: low halves plus carry fit in three 32-bit chunks.
    let m = (p01 & M) + (p10 & M) + k0;
    let l1 = m & M;
    let km = m >> 32; // <= 2

    // High halves of the middle products plus that carry.
    let mh = (p01 >> 32) + (p10 >> 32) + km; // fits u64

    // Fold the high word of c into the middle column.
    let t = (c >> 32) + l1; // <= 2*(2^32 - 1)
    let l1f = t & M;
    let kt = t >> 32; // <= 1

    let hi = p11 + mh + kt;
    let lo = (l1f << 32) | l0;
    (hi, lo)
}

/// `mul_u64_add_u64_div_u64()`: computes `floor((a * b + c) / d)` with a
/// 128-bit intermediate product, using the long-division algorithm from
/// `lib/math/div64.c` (`BITS_PER_ITER == 32` configuration): normalize by
/// left-aligning the divisor, iterate over 32-bit quotient digits estimated
/// from the divisor's most significant bits, correcting each digit estimate
/// (the guestimate can be low by up to two), plus a final correction step.
///
/// Contract (see module docs):
/// * `d == 0` **panics** (C: deliberate divide-by-zero exception).
/// * If the true quotient exceeds `u64::MAX` (`a*b + c >= 2^64` and the
///   128-bit high half is `>= d`), returns [`MUL_OVERFLOW`] — this is C's
///   *defined* `~0ULL` saturation, not UB.
#[must_use]
pub fn mul_u64_add_u64_div_u64(a: u64, b: u64, c: u64, d: u64) -> u64 {
    let (mut n_hi, mut n_lo) = mul_u64_u64_add_u64_chunks(a, b, c);

    if n_hi == 0 {
        // Fits in 64 bits: plain division. C reaches div64_u64() which traps
        // on d == 0; so do we.
        return div64_u64(n_lo, d);
    }

    if n_hi >= d {
        // C: trigger the runtime divide-by-zero exception if d == 0, else
        // saturate. Both paths made explicit here.
        assert!(d != 0, "mul_u64_add_u64_div_u64: division by zero");
        return MUL_OVERFLOW;
    }

    // Left align the divisor, shifting the dividend to match.
    let d_z = d.leading_zeros();
    let mut d = d;
    if d_z != 0 {
        d <<= d_z;
        n_hi = n_hi << d_z | n_lo >> (64 - d_z);
        n_lo <<= d_z;
    }

    const BITS_PER_ITER: u32 = 32; // __LONG_WIDTH__ >= 64 configuration

    // reps = 64 / BITS_PER_ITER, optimized down for small dividends.
    let mut reps: u32 = 64 / BITS_PER_ITER;
    if (n_hi >> 32) as u32 == 0 {
        reps -= 32 / BITS_PER_ITER;
        n_hi = n_hi << 32 | n_lo >> 32;
        n_lo <<= 32;
    }

    // Invert the dividend so we can use add instead of subtract.
    n_lo = !n_lo;
    n_hi = !n_hi;

    // Most significant BITS_PER_ITER bits of the (normalized) divisor, used
    // for the low 'guestimate' of each quotient digit.
    let d_msig: u64 = (d >> (64 - BITS_PER_ITER)) + 1;

    let mut quotient: u64 = 0;
    while reps > 0 {
        reps -= 1;

        // Guess the next 32-bit quotient digit; can be low by 1 or 2.
        let mut q_digit: u64 = (!n_hi >> (64 - 2 * BITS_PER_ITER)) / d_msig;

        // Shift n left to align with the product q_digit * d, tracking carry
        // out in `overflow` (C: u32).
        let mut overflow: u32 = (n_hi >> (64 - BITS_PER_ITER)) as u32;
        n_hi = (n_hi << BITS_PER_ITER).wrapping_add((n_lo >> (64 - BITS_PER_ITER)) as u32 as u64);
        n_lo <<= BITS_PER_ITER;

        // Add the product to the negated dividend; the returned high half
        // carries out beyond the u32 window.
        let (hi, _lo) = mul_u64_u64_add_u64_chunks(d, q_digit, n_hi);
        n_hi = _lo;
        overflow = overflow.wrapping_add(hi as u32);

        // Adjust for the guestimate being low (worst case: two extra steps).
        while overflow != u32::MAX {
            q_digit += 1;
            let before = n_hi;
            n_hi = n_hi.wrapping_add(d);
            overflow = overflow.wrapping_add(u32::from(n_hi < before));
        }

        quotient = quotient.wrapping_shl(BITS_PER_ITER).wrapping_add(q_digit);
    }

    // The loop only guarantees the remainder does not overflow its window;
    // it can still be possible to add (aka subtract) another copy of d.
    if n_hi.wrapping_add(d) > n_hi {
        quotient = quotient.wrapping_add(1);
    }

    quotient
}

/// `mul_u64_u64_div_u64()` (math64.h macro): `floor((a * b) / d)` with a
/// 128-bit intermediate. Panics on zero divisor; saturates to [`u64::MAX`]
/// when the true quotient is unrepresentable (C-defined behavior).
#[inline]
#[must_use]
pub fn mul_u64_u64_div_u64(a: u64, b: u64, d: u64) -> u64 {
    mul_u64_add_u64_div_u64(a, b, 0, d)
}

/// `mul_u64_u64_div_u64_roundup()` (math64.h macro): `ceil((a * b) / d)`
/// computed as `floor((a*b + d - 1) / d)`. Panics on zero divisor (the C
/// macro computes `d - 1`, which wraps silently for `d == 0`).
#[inline]
#[must_use]
pub fn mul_u64_u64_div_u64_roundup(a: u64, b: u64, d: u64) -> u64 {
    assert!(d != 0, "mul_u64_u64_div_u64_roundup: division by zero");
    mul_u64_add_u64_div_u64(a, b, d - 1, d)
}

/// `mul_u64_u32_add_u64_shr()` (vdso/math64.h): `((a * mul + b) >> shift)`
/// with 128-bit intermediate, truncating to 64 bits.
///
/// Precondition `shift < 64` (a larger shift is UB in C); asserted in debug
/// builds.
#[inline]
#[must_use]
pub const fn mul_u64_u32_add_u64_shr(a: u64, mul: u32, b: u64, shift: u32) -> u64 {
    debug_assert!(shift < 64, "mul_u64_u32_add_u64_shr: shift >= 64 is UB in C");
    ((((a as u128) * mul as u128).wrapping_add(b as u128)) >> shift) as u64
}

// ---------------------------------------------------------------------------
// Binary GCD (lib/math/gcd.c)
// ---------------------------------------------------------------------------

/// `gcd()`: greatest common divisor of two `u64`s (C `unsigned long` on
/// LP64 targets), using the binary GCD (Stein) algorithm's ffs-accelerated
/// branch, which the C source selects when efficient `__ffs` exists.
///
/// `gcd(0, b) == b`, `gcd(a, 0) == a` (defined in C via the early return of
/// `a | b`).
#[must_use]
pub fn gcd(a: u64, b: u64) -> u64 {
    let r = a | b;
    if a == 0 || b == 0 {
        return r;
    }
    binary_gcd_ffs(a, b)
}

/// The ffs-accelerated even/odd-pairing binary GCD body
/// (`static binary_gcd()` in gcd.c). Both inputs must be non-zero.
fn binary_gcd_ffs(mut a: u64, mut b: u64) -> u64 {
    let r = a | b;

    b >>= ffs64(b);
    if b == 1 {
        return r & r.wrapping_neg(); // isolate least significant set bit
    }

    loop {
        a >>= ffs64(a);
        if a == 1 {
            return r & r.wrapping_neg();
        }
        if a == b {
            return a << ffs64(r);
        }

        if a < b {
            core::mem::swap(&mut a, &mut b);
        }
        a -= b;
    }
}

/// The even/odd normalization fallback branch of C `gcd()` (selected when the
/// platform lacks efficient `__ffs`). Provided as a public sibling so both C
/// configurations have a named implementation; [`gcd_test_consistency`]
/// (tests) proves the two agree.
///
/// `gcd_even_odd(0, b) == b`, `gcd_even_odd(a, 0) == a`.
#[must_use]
pub fn gcd_even_odd(mut a: u64, mut b: u64) -> u64 {
    let mut r = a | b;

    if a == 0 || b == 0 {
        return r;
    }

    // Isolate least significant set bit of r.
    r &= r.wrapping_neg();

    while b & r == 0 {
        b >>= 1;
    }
    if b == r {
        return r;
    }

    loop {
        while a & r == 0 {
            a >>= 1;
        }
        if a == r {
            return r;
        }
        if a == b {
            return a;
        }

        if a < b {
            core::mem::swap(&mut a, &mut b);
        }
        a -= b;
        a >>= 1;
        if a & r != 0 {
            a += b;
        }
        a >>= 1;
    }
}

// ---------------------------------------------------------------------------
// Reciprocal division (lib/math/reciprocal_div.c + include/linux/reciprocal_div.h)
// ---------------------------------------------------------------------------

/// `struct reciprocal_value`: multiplier/shift pair for fast division by an
/// invariant `u32` divisor (Granlund & Montgomery, Figure 4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReciprocalValue {
    /// Magic multiplier.
    pub m: u32,
    /// First shift amount (`min(l, 1)` in C).
    pub sh1: u8,
    /// Second shift amount (`max(l - 1, 0)` in C).
    pub sh2: u8,
}

/// `reciprocal_value()`: precompute the reciprocal of `d`.
///
/// Panics on `d == 0` (C: `do_div` by zero trap).
#[must_use]
pub fn reciprocal_value(d: u32) -> ReciprocalValue {
    assert!(d != 0, "reciprocal_value: division by zero");

    let l = fls32(d - 1);
    // m = (2^32 * (2^l - d)) / d + 1, computed in u64 exactly as C does
    // before its (lossless here) truncation to u32.
    let m = (1u64 << 32).wrapping_mul((1u64 << l) - u64::from(d)) / u64::from(d) + 1;

    ReciprocalValue {
        m: m as u32,
        sh1: l.min(1) as u8,
        sh2: l.saturating_sub(1) as u8,
    }
}

/// `reciprocal_divide()` (inline in reciprocal_div.h): divide `a` by the
/// precomputed reciprocal `R` using two multiplies/shifts instead of a
/// division instruction.
#[inline]
#[must_use]
pub fn reciprocal_divide(a: u32, r: ReciprocalValue) -> u32 {
    let t = (((u64::from(a)) * u64::from(r.m)) >> 32) as u32;
    (t.wrapping_add(a.wrapping_sub(t) >> r.sh1)) >> r.sh2
}

/// `struct reciprocal_value_adv`: advanced reciprocal (Granlund & Montgomery,
/// Figure 4.2) as used for JIT divide emulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReciprocalValueAdv {
    /// Magic multiplier (may conceptually exceed 32 bits; see `is_wide_m`).
    pub m: u32,
    /// Post-multiplier shift.
    pub sh: u8,
    /// `ceil(log2(d))` exponent.
    pub exp: u8,
    /// True when the mathematical multiplier needs more than 32 bits.
    pub is_wide_m: bool,
}

/// `reciprocal_value_adv()`: precompute the advanced reciprocal of `d` with
/// the given precision.
///
/// Returns [`Option::None`] when `ceil(log2(d)) == 32`, i.e. `d >= 2^31`:
/// the C code `WARN`s and then performs an out-of-range `1ULL << 64` shift,
/// which the header explicitly declares unsupported ("doesn't support such
/// divisor"); we refuse instead of returning garbage.
#[must_use]
pub fn reciprocal_value_adv(d: u32, prec: u8) -> Option<ReciprocalValueAdv> {
    assert!(d != 0, "reciprocal_value_adv: division by zero");

    // ceil(log2(d))
    let l = fls32(d - 1);
    if l == 32 {
        return None; // C: WARN() + invalid shift; unsupported by contract.
    }

    let mut post_shift = l;
    let mut mlow = (1u64 << (32 + l)) / u64::from(d);
    // C: mhigh = (1ULL << (32 + l)) + (1ULL << (32 + l - prec)). When
    // prec > 32 + l that shift amount is negative — UB in C (the header's
    // documented usages never do this), so the bump term clamps to 0 here.
    let bump = if u32::from(prec) <= 32 + l {
        1u64 << (32 + l - u32::from(prec))
    } else {
        0
    };
    let mut mhigh = ((1u64 << (32 + l)) + bump) / u64::from(d);

    debug_assert!(
        u32::from(prec) <= 32 + l,
        "reciprocal_value_adv: prec > 32 + ceil(log2(d)) is UB in C"
    );

    while post_shift > 0 {
        let lo = mlow >> 1;
        let hi = mhigh >> 1;

        if lo >= hi {
            break;
        }

        mlow = lo;
        mhigh = hi;
        post_shift -= 1;
    }

    Some(ReciprocalValueAdv {
        m: mhigh as u32,
        sh: post_shift as u8,
        exp: l as u8,
        is_wide_m: mhigh > u64::from(u32::MAX),
    })
}

/// Fast-path division using an advanced reciprocal, transcribing the usage
/// pseudo-code documented at the top of `include/linux/reciprocal_div.h`
/// (the non-power-of-two branches; callers who know `d == 1 << exp` should
/// just shift by `exp`, as the header instructs JITs to do).
///
/// `pre_shift` is the factorization shift from the header recipe
/// (`floor(log2(d & -d))`) applied only when required; it must be zero when
/// `is_wide_m` is set (debug-asserted, as the pseudo-code notes).
#[inline]
#[must_use]
pub fn divide_with_reciprocal_adv(n: u32, rv: ReciprocalValueAdv, pre_shift: u8) -> u32 {
    if rv.is_wide_m {
        // pre_shift must be zero when reached here.
        debug_assert_eq!(pre_shift, 0, "wide-m reciprocal requires pre_shift == 0");
        let t = (((u64::from(n)) * u64::from(rv.m)) >> 32) as u32;
        let mut result = n.wrapping_sub(t);
        result >>= 1;
        result = result.wrapping_add(t);
        result >> (rv.sh - 1)
    } else {
        let mut result = if pre_shift != 0 { n >> pre_shift } else { n };
        result = (((u64::from(result)) * u64::from(rv.m)) >> 32) as u32;
        result >> rv.sh
    }
}

/// Header recipe for choosing `pre_shift` (and re-deriving the reciprocal)
/// when the multiplier would be wide and the divisor is even:
/// `pre_shift = floor(log2(d & -d))` (= index of lowest set bit).
/// Returns `None` when the recipe says no pre-shift is used.
#[must_use]
pub fn adv_pre_shift(d: u32, rv: ReciprocalValueAdv) -> Option<u8> {
    if rv.is_wide_m && (d & 1) == 0 {
        Some((31 - (d & d.wrapping_neg()).leading_zeros()) as u8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod verify;
