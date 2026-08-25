# Rewrite Tracker — shared state for the agent team

> **Protocol:** Every agent MUST update its row(s) here when status changes.
> One row per port/task. Never delete rows; mark `abandoned` with reason.
> Update via the `tracker_update` pi tool (or edit + re-run `ci/check.sh`).

Status vocabulary: `backlog` → `spec` → `porting` → `testing` → `review`
→ `ready` → `submitted` → `merged` | `blocked` | `abandoned`.

| ID | Target (file/dir) | Phase | Owner agent | Status | Layers done | unsafe % | Notes |
|----|-------------------|-------|-------------|--------|-------------|----------|-------|
| P-000 | pilot driver (TBD) | 1 | unassigned | backlog | — | — | pick from drivers/char or drivers/misc |
| A-000 | rewrite infra | 0 | ox-alpha | merged | unit,prop | n/a | this directory |

## Layer codes
`unit` = KUnit/doctests · `prop` = property tests · `kani` = model checking ·
`loom`/`shuttle` = concurrency · `fuzz` = syzkaller/libfuzzer ≥24h clean ·
`san` = KASAN/KCSAN/lockdep visibility verified

## Blockers log

| Date | ID | Blocker | Escalated to |
|------|----|---------|--------------|
