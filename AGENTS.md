# AGENTS.md

Guidance for AI coding agents working in the Linux kernel tree.

## Mandatory reading first

The repo README requires AI assistants to follow `Documentation/process/coding-assistants.rst`. Its rules below are hard requirements, not suggestions.

## Rust rewrite program (if working on a `rw/*` branch or a TRACKER task)

The Rust rewrite effort has its own infrastructure and agent-team protocols in `rewrite/`. Before touching any port:
1. Read `rewrite/README.md`, then `rewrite/agents/TEAM.md` for your role.
2. Load your role skill: `/skill:kernel-port`, `/skill:kernel-review`, `/skill:kernel-test-harness`, or `/skill:kernel-verify`.
3. Use the registered tools (`kernel_build`, `kernel_checkpatch`, `safety_audit`, `kunit_run`, `get_maintainer`, `tracker_update`) instead of ad-hoc shell commands.
4. All status changes go through `rewrite/TRACKER.md` via the `tracker_update` tool.

## Hard rules (legal/process)

- **Never add a `Signed-off-by:` tag.** Only humans can certify the DCO.
- **Add an `Assisted-by: LLM [tools]` tag** to commit messages you author (e.g., `Assisted-by: LLM coccinelle sparse`; do not list basic tools like git/make).
- When finding bugs: verify with a reproducer before reporting, always attempt the fix in the same session, and state explicitly anything you could not build/test/reproduce. See the procedure in `coding-assistants.rst`.
- The assistant never sends patches itself; leave submission to the human.

## Build & verify

```sh
make defconfig && make -j$(nproc)          # x86_64 default build
make ARCH=arm64 LLVM=1 -j$(nproc)          # cross-compile with Clang/LLVM
make O=build -j$(nproc)                    # out-of-tree build keeps source clean
make coccicheck                            # Coccinelle semantic checks
make htmldocs                              # build documentation
```

- A clean build takes a while; prefer incremental rebuilds and out-of-tree (`O=`) output dirs.
- New code must compile without introducing warnings; `make W=1` catches more.

## Style checks & review tooling

- Before finishing any change: `./scripts/checkpatch.pl --strict <patch>` or on commits: `./scripts/checkpatch.pl -g HEAD~..HEAD`.
- Indentation is tabs (8-char display); preferred line length 80 columns (checkpatch default max 100).
- Find reviewers/lists per file: `./scripts/get_maintainer.pl <patch-or-file>`. Ownership is defined by the top-level `MAINTAINERS` file.

## Testing

- KUnit (in-kernel unit tests): `./tools/testing/kunit/kunit.py run`, optionally `--kunitconfig lib/<subdir>` for focused suites.
- kselftest: `make kselftest` (use `KBUILD_OUTPUT=/tmp/kselftest` to avoid polluting the tree); individual tests under `tools/testing/selftests/`.
- Boot-testing usually isn't possible here; say so instead of claiming runtime verification.

## Conventions that differ from generic projects

- Commit subjects: `<subsystem>: <summary>` (e.g., `net: ...`, `drm/i915: ...`), imperative mood, no markdown, wrap at ~72 columns. Detailed body explaining *why*, not just what.
- Bug fixes need a `Fixes:` tag: `Fixes: <first 12 chars of SHA> ("<original commit subject>")`.
- Never break userspace: changes to UAPI headers (`include/uapi/`) are ABI-sensitive and require strong justification.
- Do not reformat unrelated code; keep diffs minimal even if surrounding style looks off.
- SPDX license identifiers required on new files (`GPL-2.0` family); see `Documentation/process/license-rules.rst`.
