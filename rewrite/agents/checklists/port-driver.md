# Checklist: C→Rust Driver Port

Run top to bottom; do not skip ahead. Mirrors PROTOCOLS.md §7.

## 0. Setup
- [ ] TRACKER row created, status `spec`, branch `rw/<ID>-<slug>` cut
- [ ] Read the C driver fully; note LOC, ioctls, locks, ISRs, workqueues

## 1. Semantic spec (`specs/<ID>.md`)
- [ ] Observable behavior documented
- [ ] Locking inventory complete (what each lock protects; ordering)
- [ ] All error paths + userspace-visible results listed
- [ ] UAPI surface touched: confirmed EMPTY
- [ ] Deliberate C-quirk compatibility list written

## 2. Translation
- [ ] Device/driver plumbing via kernel crate idioms (pci/platform/device::Driver)
- [ ] Data wrapped in the right smart pointers (ARef/KBox/etc.) — no raw refcount games
- [ ] Locks 1:1 with C first; deviations require Verifier involvement
- [ ] Every `unsafe` has a SAFETY comment (audit passes)
- [ ] No panics on reachable paths from userspace input (return Err instead)
- [ ] Error conversion preserves errno semantics exactly
- [ ] Module init/exit order matches C (probe/remove, suspend/resume hooks)

## 3. Self-checks before handoff to testing
- [ ] `make -C rewrite fmt lint audit` green
- [ ] Builds x86_64 AND arm64 out-of-tree
- [ ] checkpatch --strict clean on your commits
- [ ] Commit messages follow format (no Signed-off-by!, Assisted-by present)

## 4. Handoff
- [ ] Spec committed; TRACKER status → `testing`; notes updated
