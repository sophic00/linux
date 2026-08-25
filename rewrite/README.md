# Rust Rewrite Program

Infrastructure for incrementally rewriting Linux kernel components in Rust,
executed by a team of AI agents under human supervision.

**Read this order:** `PLAN.md` → `agents/TEAM.md` → `agents/PROTOCOLS.md`.

## Layout

| Path | Purpose |
|---|---|
| `PLAN.md` | Phased roadmap, risks, definition-of-done |
| `TRACKER.md` | Shared state: per-subsystem/port status (agents must keep updated) |
| `Makefile` | Convenience targets wrapping everything below |
| `ci/check.sh` | Full gate matrix: fmt, clippy, builds (x86_64/arm64), rustdoc, unit tests |
| `ci/safety-audit.py` | `unsafe`-block audit: every `unsafe` needs a `// SAFETY:` comment |
| `testing/kunit/` | In-kernel unit/doctest configuration fragments |
| `testing/property/` | Property-based testing: host-crate pattern + runnable example |
| `testing/fuzzing/` | Coverage-guided fuzzing: syzkaller config, harness checklist |
| `verification/` | Kani model checking, Miri UB detection, Loom concurrency testing |
| `agents/` | Team structure, protocols, per-task checklists |

## Quickstart

```sh
cd rewrite
make help          # list targets
make audit         # fast: safety audit only
make check         # full gate matrix (skips gracefully if tooling missing)
cargo test --manifest-path testing/property/kernel-logic-hosttests/Cargo.toml
```

## Testing pyramid (all layers are mandatory)

1. **Unit/doctests** — every Rust item gets doc examples that double as KUnit
   tests (`CONFIG_RUST_KERNEL_DOCTESTS`). Run: `make unit`, `make kunit`.
2. **Property-based tests** — pure logic extracted to `no_std`-compatible
   crates, exercised with thousands of generated cases on the host.
   See `testing/property/`.
3. **Model checking** — Kani proves contracts over *all* inputs for critical
   `unsafe` code. See `verification/kani/`.
4. **Concurrency exploration** — Loom/Shuttle for any locking/atomics
   abstraction. See `verification/loom/`.
5. **Coverage-guided fuzzing** — syzkaller + KCOV against converted drivers,
   KASAN/KCSAN/KFENCE enabled. See `testing/fuzzing/`.
6. **Runtime sanitizers** — mixed C/Rust builds must stay visible to
   KASAN/KCSAN/KMSAN/lockdep. Verify per-port, see checklists.

## Hard rules (inherited from AGENTS.md)

- AI agents NEVER add `Signed-off-by:` — human integrator only (DCO).
- Every AI-authored commit carries `Assisted-by: LLM [tools]`.
- UAPI (`include/uapi/`, `rust/uapi/`) is frozen; changes escalate to human.
- Bug fixes require a `Fixes:` tag referencing the offending commit.
