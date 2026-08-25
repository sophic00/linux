# Verification Layer Overview

Static/formal verification complements testing: tests sample the input space,
verification covers it exhaustively. Use in this order per port:

| Tool | Covers | Scope | Cost | When |
|---|---|---|---|---|
| `cargo clippy` | lint-level bugs | all code | seconds | every build (`CLIPPY=1`) |
| `safety-audit.py` | unsafe hygiene | rust/ tree | seconds | every commit |
| Kani | full input space for contracts | pure logic + unsafe blocks | minutes–hours | every `unsafe`, every core algorithm |
| Miri | UB in pure-Rust code | no-FFI modules | slow | nightly CI on hosttests crate |
| Loom/Shuttle | all interleavings | concurrency primitives | exhaustive, small models only | every new lock/atomic abstraction |

Sub-pages:
- `kani/README.md` — model checking setup + harness conventions
- `miri/run_miri.sh` — UB detection runner
- `loom/README.md` — concurrency exploration pattern

## Rules

1. No `unsafe` block lands without either a Kani proof of its contract or an
   explicit human sign-off recorded in TRACKER.md.
2. Concurrency abstractions land only after a Loom model passes.
3. Verification artifacts (harnesses) live *next to the code they verify*,
   guarded by `#[cfg(kani)]` / `#[cfg(loom)]` so normal builds are unaffected.
