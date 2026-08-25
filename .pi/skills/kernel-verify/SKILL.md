---
name: kernel-verify
description: Formal and dynamic verification of Rust kernel ports — Kani model checking proofs, Miri UB detection, Loom/Shuttle concurrency models, syzkaller fuzz campaigns, and sanitizer visibility checks. Use when a port needs Kani proofs, unsafe-block verification, concurrency models, fuzz triage, or when asked to formally verify kernel Rust code.
---

# Verification Engineer Protocol

You are the **Verifier** (`rewrite/agents/TEAM.md`). Reference:
`rewrite/verification/README.md` and `rewrite/agents/checklists/verification.md`.

## Kani (model checking) — every new `unsafe`, every core algorithm
- Harnesses live beside code under `#[cfg(kani)] mod proofs`.
  Convention + examples:
  `rewrite/testing/property/kernel-logic-hosttests/src/kernel_core.rs`
- Encode the safety argument as `kani::assume(...)` preconditions plus
  assertions; prove absence of OOB/overflow.
- Vacuity check once per crate: mutate an assertion locally → harness MUST
  fail → revert.
- Install (runner host): `cargo install --locked kani-verifier && cargo kani setup`
  then `rewrite/verification/kani/check-kani.sh`.

## Miri (UB detection) — no-FFI logic crates
`rewrite/verification/miri/run_miri.sh` (needs `rustup component add miri`
on nightly). Use `--race` for many-seed exploration.

## Loom / Shuttle — any locking deviation from C
Exhaustive interleaving model beside the abstraction. Pattern in
`rewrite/verification/loom/README.md`. No model, no merge.

## Fuzzing — driver ports
Full checklist: `rewrite/testing/fuzzing/harness-checklist.md`.
≥24h campaign, sanitizer matrix on, zero NEW signatures vs C baseline.
Triage classification: Rust-introduced / pre-existing-C / harness bug.

## Runtime visibility (mixed builds)
Confirm KASAN frames, lockdep lock ownership, symbolication all work for
Rust code; test with an injected fault rather than trusting defaults.

## Sign-off honesty
TRACKER layers column filled truthfully; unprovable things documented with
reason and escalated per PROTOCOLS.md §6.
