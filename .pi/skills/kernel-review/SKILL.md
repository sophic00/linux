---
name: kernel-review
description: Review a Rust kernel port for semantic equivalence with the original C code, hygiene gates, and process compliance. Use when a TRACKER row enters 'review', when asked to review a kernel patch series, or as pre-screening before human review.
---

# Kernel Port Review Protocol

You are the **Reviewer** (see `rewrite/agents/TEAM.md`). You must NOT be the
agent that wrote this code. Work in stages; reject at first failed stage.

## Stage 1 — Spec vs C (never skip)
Re-read the C source yourself. Diff against `rewrite/specs/<ID>.md`.
Missing behavior, missing lock ordering, wrong errno = instant reject.

## Stage 2 — Code vs Spec
- Lock scheme identical to C (spinlock vs mutex placement, IRQ context)
- No invented behavior; error paths preserve userspace-visible semantics
- Drop paths never sleep holding spinlocks; refcounts via ARef etc., not raw

## Stage 3 — Gates (run, don't trust claims)
```
kernel_build x86_64 + arm64      (tool)
safety_audit                     (tool)
kernel_checkpatch <range>        (tool)
make -C rewrite fmt lint         (bash)
git diff --stat -- include/uapi rust/uapi   # must be empty
```

## Stage 4 — Testing artifacts
Verify KUnit results, property tests ≥10k cases, Kani proofs non-vacuous
(mutate one assertion → harness must fail), fuzz log ≥24h for drivers.

## Verdict
Approve: TRACKER → `ready` with date. Reject: actionable notes in TRACKER,
status back one stage. Be specific and terse.
