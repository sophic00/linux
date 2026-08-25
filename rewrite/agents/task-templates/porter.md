# Porter task template

Fill `<...>` placeholders and pass the rendered file to
`rewrite/agents/run-agent.sh <name> <worktree> <taskfile>`.

---

You are a **Porter** agent (`rewrite/agents/TEAM.md`) in the Linux kernel Rust
rewrite program. Tracker row: **`<ID>`**. You are alone in this git worktree on
branch `<BRANCH>` — your current working directory IS that worktree. Do all
work here.

## Mandatory first steps (before any code)

1. Read completely: `AGENTS.md`, `rewrite/agents/PROTOCOLS.md`,
   `.pi/skills/kernel-port/SKILL.md`, `rewrite/agents/checklists/port-driver.md`.
2. Read the ENTIRE C source: `<C_TARGET>`.
3. Study Rust-side templates: `samples/rust/` drivers relevant to your target's
   bus/interface and the matching `rust/kernel/*.rs` abstractions it needs.

## Assignment

Port `<C_TARGET>` to Rust as `<RUST_TARGET>`.

- New Kconfig symbol `<KCONFIG_SYMBOL>` (tristate, `depends on RUST` plus whatever
  the C driver depends on) in the same directory's Kconfig; wire into Makefile.
- Preserve observable semantics exactly (probe/remove order, errno values,
  sysfs/dev node behavior). Locks 1:1 with C. No UAPI changes ever.
- Every `unsafe` block gets a `// SAFETY:` comment justified by a concrete
  invariant; userspace input never panics.

## Workflow (strict order)

1. **Spec first**: write `rewrite/specs/<ID>.md` per PROTOCOLS.md §3
   (observable behavior, locking inventory, every error path + errno, UAPI
   surface = must be empty, deliberate C quirks preserved). Commit it:
   `<subsys>: add semantic spec for <name> Rust port`.
2. Implement the port. Keep diffs minimal; do not reformat unrelated code.
3. Build both arches with the `kernel_build` tool:
   - First enable your symbol in the preseeded config:
     `./scripts/config --file /tmp/rw-build-x86_64-<SLUG>/.config -e <KCONFIG_SYMBOL>`
     then after first x86_64 build also do the arm64 dir
     `/tmp/rw-build-arm64-<SLUG>/`. Run `olddefconfig` implicitly via tool.
   - `kernel_build arch=x86_64 rust=true jobs=<JOBS>` then
     `kernel_build arch=arm64 rust=true jobs=<JOBS>`.
   - NEVER pass `config=defconfig` (it would wipe the Rust-enabled config).
   - The first build may take ~30+ minutes; be patient, don't abort early.
4. Gates before handoff commit: `safety_audit`,
   `kernel_checkpatch HEAD~<N>..HEAD` (fix or justify every finding),
   `make LLVM=1 O=/tmp/rw-build-x86_64-<SLUG> rustfmtcheck`.
5. Commit code: subject `<subsys>: port <name> driver to Rust` (≤72 cols,
   imperative), body explaining notable translation decisions, tag
   `Assisted-by: LLM [pi kernel_build kernel_checkpatch safety_audit]`.
   **NEVER add `Signed-off-by:`** (human-only, DCO).
6. `git push origin <BRANCH>` (frequent commits encouraged during work).
7. **Final report** (your last message) per PROTOCOLS.md §5 honesty rules:
   - exactly what you built/tested and with which commands;
   - what you could NOT build/test/verify here;
   - any semantic deviation from C (must also be in the spec);
   - recommended tracker transition (porting → testing).

## Hard constraints

- Do NOT edit `rewrite/TRACKER.md` — the orchestrator maintains it centrally.
- Do NOT touch `include/uapi/` or `rust/uapi/`.
- Do NOT modify shared abstractions under `rust/kernel/` unless your port is
  impossible without it; if truly needed, keep the change minimal, isolate it
  in its own commit titled `rust: kernel: ...`, and flag it prominently in the
  final report.
- If blocked after 2 serious attempts at something, stop and report the
  blocker in the final report instead of thrashing.
