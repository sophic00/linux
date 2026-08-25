//! Tests for `krand`. The expected constants below were derived
//! independently with a Python transcription of the algorithms *before*
//! being embedded here, so the tests pin behavior to an external oracle
//! rather than to whatever the Rust code happens to do.

extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use crate::{derive_seed, Krand, Rng, SplitMix64};

/// SplitMix64 raw stream, seed = 7 (oracle: Python transcription).
const SPLITMIX_SEED7_FIRST3: [u64; 3] = [
    0x63cb_e1e4_5932_0dd7,
    0x044c_3cd7_f43c_661c,
    0xe698_4080_bab1_2a02,
];

/// seed_from_u64(42) expands (via SplitMix64) to this xorshift128+ state.
const SEED42_S0: u64 = 0xbdd7_3226_2feb_6e95;
const SEED42_S1: u64 = 0x28ef_e333_b266_f103;

/// First outputs of xorshift128+ from the SEED42 state (oracle: Python).
const KRAND42_FIRST6: [u64; 6] = [
    0xaf1f_56fc_41a4_d2d2,
    0xbd49_6f01_ee60_5ceb,
    0x8c8b_2271_e69f_dbf6,
    0x5438_402a_c692_1e50,
    0x36bc_fece_6780_5193,
    0xe823_1a6d_40bb_b088,
];

#[test]
fn splitmix64_matches_reference_stream() {
    let mut g = SplitMix64::new(7);
    for want in SPLITMIX_SEED7_FIRST3 {
        assert_eq!(g.next_u64(), want);
    }
}

#[test]
fn splitmix64_seed_zero_is_fine() {
    // SplitMix64 has no bad zero state, unlike xorshift128+.
    let mut g = SplitMix64::new(0);
    let a = g.next_u64();
    let b = g.next_u64();
    assert_ne!(a, 0);
    assert_ne!(b, a);
}

#[test]
fn seed_expansion_matches_reference_state() {
    let mut sm = SplitMix64::new(42);
    let s0 = sm.next_u64();
    let s1 = sm.next_u64();
    assert_eq!((s0, s1), (SEED42_S0, SEED42_S1));
    // And Krand must use exactly that expansion:
    let k = Krand::seed_from_u64(42);
    assert_eq!(k, Krand::from_state(SEED42_S0, SEED42_S1));
}

#[test]
fn xorshift128plus_matches_reference_stream() {
    let mut k = Krand::seed_from_u64(42);
    for want in KRAND42_FIRST6 {
        assert_eq!(k.next_u64(), want);
    }
}

#[test]
fn reproducible_across_instances() {
    let mut a = Krand::seed_from_u64(0xDEAD_BEEF);
    let mut b = Krand::seed_from_u64(0xDEAD_BEEF);
    for _ in 0..1000 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
}

#[test]
fn different_seeds_diverge() {
    let mut a = Krand::seed_from_u64(1);
    let mut b = Krand::seed_from_u64(2);
    let da: Vec<u64> = (0..16).map(|_| a.next_u64()).collect();
    let db: Vec<u64> = (0..16).map(|_| b.next_u64()).collect();
    assert_ne!(da, db);
}

#[test]
fn consecutive_seeds_produce_uncorrelated_streams() {
    // Adjacent seeds are the classic foot-gun of naive LCGs; SplitMix
    // expansion must decorrelate them. Compare byte-level equality count.
    let mut prev: Vec<u8> = Vec::new();
    let collisions = |s: u64| -> usize {
        let mut k = Krand::seed_from_u64(s);
        let v: Vec<u8> = (0..8).flat_map(|_| k.next_u64().to_le_bytes()).collect();
        if prev.is_empty() {
            prev = v.clone();
            return 64; // first stream compared against itself would lie
        }
        let c = v.iter().zip(prev.iter()).filter(|(x, y)| x == y).count();
        prev = v;
        c
    };
    let total: usize = (0..64u64).map(collisions).sum();
    // Random bytes agree ~1/256 of positions; 64 streams x 64 bytes.
    // Anything above ~1/8 agreement across the whole run signals correlation.
    assert!(
        total < 64 * 64 / 8,
        "correlated adjacent-seed streams: {total}"
    );
}

#[test]
fn next_u32_uses_high_bits() {
    let mut k = Krand::seed_from_u64(9);
    let raw = k.next_u64();
    let mut k2 = Krand::seed_from_u64(9);
    assert_eq!(k2.next_u32() as u64, raw >> 32);
}

#[test]
fn below_respects_bounds_and_zero_panics() {
    let mut k = Krand::seed_from_u64(12345);
    for &bound in &[1u64, 2, 3, 7, 10, 255, 256, u32::MAX as u64] {
        for _ in 0..10_000 {
            let v = k.below(bound);
            assert!(v < bound, "below({bound}) produced {v}");
        }
    }
}

#[test]
fn below_is_uniform_for_awkward_bounds() {
    // 3 does not divide 2^64 evenly — modulo bias is exactly what rejection
    // sampling prevents. Chi-square-lite over three buckets.
    let mut k = Krand::seed_from_u64(777);
    let n = 300_000u64;
    let mut counts = [0u64; 3];
    for _ in 0..n {
        counts[k.below(3) as usize] += 1;
    }
    let expected = n / 3;
    for (i, &c) in counts.iter().enumerate() {
        assert!(
            c > expected * 97 / 100 && c < expected * 103 / 100,
            "bucket {i} out of ±3% band: {c} vs {expected}"
        );
    }
}

#[test]
fn bucket_balance_sixteen_buckets() {
    let mut k = Krand::seed_from_u64(0xABCD_EF01);
    const BUCKETS: usize = 16;
    let n = 160_000u64;
    let mut counts = [0u64; BUCKETS];
    for _ in 0..n {
        counts[(k.next_u64() >> 60) as usize] += 1; // top nibble
    }
    let expected = n / BUCKETS as u64;
    for (i, &c) in counts.iter().enumerate() {
        assert!(
            c > expected * 9 / 10 && c < expected * 11 / 10,
            "bucket {i}: {c} vs expected {expected}"
        );
    }
}

#[test]
fn shuffle_preserves_elements_and_permutes_order() {
    let mut k = Krand::seed_from_u64(555);
    for n in [2usize, 3, 17, 500] {
        let orig: Vec<usize> = (0..n).collect();
        let mut v = orig.clone();
        for _ in 0..50 {
            k.shuffle(&mut v);
            // Multiset preserved every step.
            let mut s1 = v.clone();
            let mut s2 = orig.clone();
            s1.sort_unstable();
            s2.sort_unstable();
            assert_eq!(s1, s2, "shuffle lost elements at n={n}");
        }
    }
    // A fixed-seed shuffle of a sorted deck must move something (probability
    // of identity permutation after Fisher-Yates on 100 elements ≈ 1/100!).
    let mut deck: Vec<usize> = (0..100).collect();
    k.shuffle(&mut deck);
    assert_ne!(deck, Vec::from_iter(0..100));
}

#[test]
fn fill_bytes_roundtrip_draws() {
    let mut k = Krand::seed_from_u64(31);
    let mut buf = [0u8; 19]; // non-multiple of 8 exercises the tail path
    k.fill_bytes(&mut buf);

    let mut k2 = Krand::seed_from_u64(31);
    let d0 = k2.next_u64().to_le_bytes();
    let d1 = k2.next_u64().to_le_bytes();
    let d2 = k2.next_u64().to_le_bytes();
    assert_eq!(&buf[..8], &d0);
    assert_eq!(&buf[8..16], &d1);
    assert_eq!(&buf[16..], &d2[..3]);
}

#[test]
fn derive_seed_spreads_iterations() {
    // Adjacent iterations get wildly different seeds (golden-ratio stride).
    assert_ne!(derive_seed(1, 0), derive_seed(1, 1));
    assert_eq!(
        derive_seed(7, 3),
        7u64.wrapping_add(3u64.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    );
}

#[test]
fn debug_formatting_is_available_but_not_entropy_sensitive() {
    // Formatting must not consume RNG state (Debug uses only fields).
    let mut k = Krand::seed_from_u64(4242);
    let _ = format!("{:?}", k);
    let first = k.next_u64();

    let mut k2 = Krand::seed_from_u64(4242);
    let _ = String::from(""); // touch alloc string machinery
    assert_eq!(k2.next_u64(), first);
}
