# Kani Model Checking

[Kani](https://github.com/model-checking/kani) proves assertions hold for
**every** input — the strongest guarantee available for pure Rust logic and
the soundness argument of `unsafe` blocks.

## Install (runner host)

```sh
cargo install --locked kani-verifier
cargo kani setup
```

## Harness conventions in this program

Proof harnesses live beside the code, guarded so kbuild never sees them:

```rust
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn contract_example() {
        let x: u32 = kani::any();
        kani::assume(x >= 1);
        assert!(roundup_pow2(x) >= x);
    }
}
```

A working set ships in
`testing/property/kernel-logic-hosttests/src/kernel_core.rs` (`mod proofs`).

## Run

```sh
./rewrite/verification/kani/check-kani.sh          # all harnesses under a crate
cargo kani --manifest-path <crate>/Cargo.toml      # directly
```

## What must be proven (per port)

1. Every pure algorithm: its documented contract as assertions.
2. Every `unsafe` block whose safety argument is input-dependent: an explicit
   harness encoding the precondition (`kani::assume(...)`) and asserting no
   out-of-bounds/arithmetic overflow can occur.
3. Any custom `unsafe impl` of a kernel trait: trait-contract harnesses.

## Limits

- FFI into C helpers is not modeled; prove the *Rust side* of the contract.
- Unbounded loops need `#[kani::unwind(n)]` with justification comments.
