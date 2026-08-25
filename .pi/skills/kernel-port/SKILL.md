---
name: kernel-port
description: Port a Linux kernel C driver/subsystem to Rust following the rewrite program's semantic-spec-first workflow. Use when assigned a TRACKER port task, when asked to translate a kernel C file to Rust, or when starting work on a rw/* branch.
---

# Kernel Port Protocol

You are a **Porter** (`rewrite/agents/TEAM.md`). One TRACKER row, one branch:
`rw/<ID>-<slug>`. You compile; you do NOT sign off testing.

## 0. Setup
- `tracker_update row_id=<ID> status=spec owner=<you>`
- Read the ENTIRE C driver. Note ioctls, locks, ISRs, workqueues, error paths.
- Checklist: `rewrite/agents/checklists/port-driver.md`

## 1. Semantic spec FIRST — no Rust before this is committed
Write `rewrite/specs/<ID>.md`: observable behavior, locking inventory
(what each lock protects + ordering), every error path with its errno,
UAPI-touched (must be EMPTY), deliberate C quirks to preserve.

## 2. Translation rules
- Idioms from `rust/kernel/` (device::Driver, pci, platform); study an
  existing converted driver in-tree as your template.
- Locks 1:1 with C first — "improvements" require the Verifier, not you.
- Refcounts via kernel smart pointers (ARef, KBox...); never raw refcount math.
- Every `unsafe` gets a `// SAFETY:` comment; userspace input never panics —
  return `Err`; errno semantics preserved exactly.
- Init/exit, probe/remove, suspend/resume ordering matches C.

## 3. Self-checks (all must pass before handoff)
```
kernel_build arch=x86_64        (tool)
kernel_build arch=arm64         (tool)
safety_audit                    (tool)
kernel_checkpatch HEAD~..HEAD   (tool)
make -C rewrite fmt lint audit  (bash)
```
Commit format: `<subsystem>: <summary>` ≤72 cols, imperative,
`Assisted-by: LLM [tools]`, **NEVER** `Signed-off-by:` (human-only, DCO).
Bug fixes need `Fixes: <12sha> ("<subject>")`.

## 4. Handoff
Commit spec + code. `tracker_update status=testing layers=` with what you
know is covered. Handoff message states exactly what you built/tested and
what you could NOT test here (PROTOCOLS.md §5 honesty rules).
