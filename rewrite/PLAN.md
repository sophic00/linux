# Rewrite Plan

## Strategy: incremental strangler-fig, never big-bang

A wholesale kernel rewrite is not executable by any team, human or AI. The only
strategy that has ever worked (and upstream's own) is converting leaves while
the C core keeps working. Order of attack, safest first:

1. Leaf drivers with narrow, well-specified hardware interfaces
2. Filesystems / block layer helpers behind stable interfaces
3. Subsystem cores once enough drivers prove the abstractions
4. Never: scheduler, core MM, entry code, architecture asm (keep C/asm)

## Phases

### Phase 0 — Infrastructure (this directory) ✅
CI gates, testing pyramid tooling, agent team definition. Done when
`make check` passes from a clean checkout.

### Phase 1 — Pilot port (1 driver, full pipeline)
Pick one small char/misc driver. Produce: semantic spec → Rust port → parity
tests → review → submission-ready series. Done when the pipeline has been
exercised end-to-end and its friction points are recorded in PROTOCOLS.md.

Exit criteria:
- [ ] Driver passes all 6 testing layers
- [ ] checkpatch clean, builds on x86_64 + arm64, W=1 clean
- [ ] Post-mortem written; checklists updated

### Phase 2 — Parallel leaf-driver campaign
Orchestrator batches independent drivers to Porter agents (see
`agents/TEAM.md`). Concurrency is safe because drivers don't share code paths;
shared *abstractions* land through a single dedicated agent to avoid conflicts.

Exit criteria per batch: TRACKER.md rows green, zero regressions in C behavior
(parity tests), no new `unsafe` without audited SAFETY comments.

### Phase 3 — Core abstraction consolidation
Promote repeated patterns from drivers into `rust/kernel/` abstractions.
Each abstraction: Loom-tested, Kani-proven core, doctested API, reviewed by
the subsystem maintainer loop (human).

## Metrics (tracked in TRACKER.md)

- Ports merged / in-flight / blocked
- `unsafe` density per port (target: <5% of lines, 100% SAFETY-commented)
- Kani proofs passing; property-test cases executed per crate (>10k)
- Fuzz uptime per driver without crashes (target: 24h+ KASAN+KCOV clean)
- Regression parity: C-vs-Rust behavioral diff suite green

## Risk register

| Risk | Mitigation |
|---|---|
| Agent ports diverge semantically from C | Mandatory semantic spec + parity tests before code (PROTOCOLS.md §3) |
| Locking subtleties lost in translation | Loom/Shuttle on every locking abstraction; reviewer agent re-derives invariants |
| UAPI breakage | Frozen; automated grep gate in ci/check.sh flags uapi diffs |
| Toolchain drift breaks tree | Pinned toolchain versions in ci/check.sh env check |
| Hallucinated APIs / nonexistent functions | Everything must compile against this tree; no external crate deps in-kernel |
| Burned-out reviewers (humans) | Reviewer agents pre-screen; humans see only pre-verified series |

## What we explicitly do NOT attempt

- Rewriting for its own sake: C code that is correct, maintained, and not being
  touched stays C.
- Any change to `include/uapi/`.
- Replacing proven C core machinery without a maintainer-driven upstream effort.
