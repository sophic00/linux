# Concurrency Verification (Loom / Shuttle)

Locking is where rewrites die. Every locking or atomic abstraction introduced
by a port MUST pass an exhaustive interleaving model before review.

## Pattern

Loom runs your code under *all possible* thread schedules (model checking),
not random ones. It requires swapping real primitives for its models:

```rust
#[cfg(loom)]
use loom::sync::{Arc, Mutex};
#[cfg(not(loom))]
use std::sync::{Arc, Mutex};

#[test]
fn spsc_no_lost_wakeups() {
    loom::model(|| {
        // minimal model: 2 threads, small state space
        // assert invariants after join
    });
}
```

Run: `RUSTFLAGS="--cfg loom" cargo test --release` (loom config in dev-deps).

## When Loom vs Shuttle vs KCSAN

| Stage | Tool | Why |
|---|---|---|
| Abstraction design | Loom | exhaustive, tiny models only (<10^7 states) |
| Integration logic | Shuttle | randomized exploration scales further |
| Whole kernel | KCSAN | runtime sampler over real workloads |

## Rules for porters

1. Port C lock usage 1:1 first; do NOT "improve" locking during a port.
2. Any deviation from the C locking scheme needs its own Loom model + human
   reviewer sign-off.
3. Kernel-side, enable CONFIG_KCSAN + CONFIG_PROVE_LOCKING in the fuzz/test
   kernels (see testing/kunit/rust-extended.kunitconfig).
