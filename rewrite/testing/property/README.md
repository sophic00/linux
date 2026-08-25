# Property-Based Testing for Kernel Rust Code

## The pattern: one crate, two worlds

Kernel code can't link `proptest`/`quickcheck` (std-only). The solution used
here — and generally applicable — is to keep *pure decision logic* in a
`#![no_std]`-compatible core module with zero kernel dependencies:

```
drivers/net/foo/           kernel crate: glue, registers, locking
  └── core.rs              pure logic: ring indices, checksums, parsers
testing/property/<crate>/  host crate: includes the same core.rs via #[path],
                           adds proptest/quickcheck generators + oracles
```

The host crate compiles the *identical source file* (`#[path = "..."] mod core;`)
so there is no drift between what is tested and what ships.

## Runnable example

`kernel-logic-hosttests/` demonstrates the pattern with zero external crates
(works offline): a tiny deterministic generator/shrinker plus properties over
power-of-two rounding and ring-buffer wraparound math — both classic bug farms.

```sh
cargo test --manifest-path testing/property/kernel-logic-hosttests/Cargo.toml
```

## Upgrading when registry access exists

Swap the built-in mini-framework for real tools (API-compatible intent):

| Crate | Use for |
|---|---|
| `proptest` | stateful property testing + shrinking |
| `quickcheck` | classic randomized properties |
| `loom` | exhaustive interleaving of concurrency primitives |
| `cargo-fuzz` | libFuzzer targets over parser core modules |

## Rules for porter agents

1. Every port must extract at least its decision logic into a pure module.
2. Minimum 3 properties per pure module, including at least one *oracle*
   (comparison against the C behavior, e.g. `roundup()` semantics).
3. ≥10,000 generated cases per property in CI (`make property`).
4. Any bug found by a property test keeps its failing case as a unit test.
