// SPDX-License-Identifier: GPL-2.0
//! Proof-of-concept differential fuzz harness for `scripts/fuzz.sh`.
//!
//! Long-running loop: shuffle a sequence with [`krand`], sort it back, and
//! verify both the permutation property and sortedness against std. Runs
//! until the time budget expires; exits nonzero on the first violated
//! property (via panic).
//!
//! Usage: `cargo run -p ktest-util --bin fuzz-permutation -- <seconds> [seed]`

extern crate alloc;

use alloc::{string::String, vec::Vec};

use krand::Rng;

fn run_one(seed: u64, n: usize) -> Result<(), String> {
    let mut rng = krand::Krand::seed_from_u64(seed);

    // Random keys + identity tags.
    let mut data: Vec<(u64, usize)> = (0..n).map(|i| (rng.next_u64() % 1000, i)).collect();
    rng.shuffle(&mut data);

    // 1) Shuffling preserved the multiset.
    let mut keys_before: Vec<u64> = data.iter().map(|&(k, _)| k).collect();
    keys_before.sort_unstable();

    // 2) Sorting restores order and total order.
    data.sort_unstable_by_key(|&(k, _)| k);
    let keys_after: Vec<u64> = data.iter().map(|&(k, _)| k).collect();
    if keys_after != keys_before {
        return Err(alloc::format!("multiset changed at n={n}"));
    }
    if !data.windows(2).all(|w| w[0].0 <= w[1].0) {
        return Err(alloc::format!("not sorted after sort at n={n}"));
    }

    // 3) Tags are a permutation of 0..n.
    let mut tags: Vec<usize> = data.iter().map(|&(_, t)| t).collect();
    tags.sort_unstable();
    if tags != Vec::from_iter(0..n) {
        return Err(alloc::format!("tag permutation broken at n={n}"));
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seconds: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    let seed: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_5EED_5EED);

    let start = std::time::Instant::now();
    let mut iters: u64 = 0;
    let mut i: u64 = 0;
    while start.elapsed().as_secs() < seconds {
        // Deterministic per-iteration seeds derived from (seed, i); sizes
        // sweep tiny (edge cases) up to a few thousand elements.
        let iter_seed = krand::derive_seed(seed, i);
        let n = (krand::Krand::seed_from_u64(iter_seed).next_u64() % 4096) as usize;
        if let Err(msg) = run_one(iter_seed, n) {
            eprintln!("FUZZ FAILURE seed={seed} iter={i}: {msg}");
            std::process::exit(1);
        }
        iters += 1;
        i += 1;
    }
    println!("fuzz-permutation: {iters} iterations OK ({seconds}s, seed={seed})");
}
