# Rewrite Tracker — shared state for the agent team

> **Protocol:** Every agent MUST update its row(s) here when status changes.
> One row per port/task. Never delete rows; mark `abandoned` with reason.
> Update via the `tracker_update` pi tool (or edit + re-run `ci/check.sh`).

Status vocabulary: `backlog` → `spec` → `porting` → `testing` → `review`
→ `ready` → `submitted` → `merged` | `blocked` | `abandoned`.

| ID | Target (file/dir) | Phase | Owner agent | Status | Layers done | unsafe % | Notes |
|----|-------------------|-------|-------------|--------|-------------|----------|-------|
| P-000 | pilot driver (TBD) | 1 | unassigned | abandoned | — | — | pick from drivers/char or drivers/misc; superseded: replaced by concrete rows P-001..P-005; pilot slot filled by P-001 open-dice |
| A-000 | rewrite infra | 0 | ox-alpha | merged | unit,prop | n/a | this directory |
| P-001 | drivers/misc/open-dice.c | 1 | porter-P-001 (subagent) | backlog | — | — | pilot port: platform+miscdevice+mutex+mmap; branch rw/P-001-open-dice |
| P-002 | drivers/misc/dummy-irq.c | 2 | unassigned | backlog | — | — | tiny IRQ-handler module; exercises irq abstraction |
| P-003 | drivers/misc/xilinx_tmr_manager.c | 2 | unassigned | backlog | — | — | MMIO + sysfs platform driver |
| P-004 | drivers/misc/xilinx_tmr_inject.c | 2 | unassigned | backlog | — | — | MMIO + sysfs platform driver |
| P-005 | drivers/char/hangcheck-timer.c | 2 | unassigned | backlog | — | — | timer + module params, no userspace ABI |

## Layer codes
`unit` = KUnit/doctests · `prop` = property tests · `kani` = model checking ·
`loom`/`shuttle` = concurrency · `fuzz` = syzkaller/libfuzzer ≥24h clean ·
`san` = KASAN/KCSAN/lockdep visibility verified

## Blockers log

| Date | ID | Blocker | Escalated to |
|------|----|---------|--------------|
