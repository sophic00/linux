# Protocols

## 1. Task lifecycle

```
backlog ──spec──► spec ──approve──► porting ──build──► testing ──harnesses──►
review ──clean──► ready ──human──► submitted ──list──► merged
        Any stage ──► blocked (reason logged) / abandoned (reason logged)
```

## 2. Branch & commit format

- Branch: `rw/<TRACKER-ID>-<slug>`
- Subject: `<subsystem>: <summary>` ≤72 cols, imperative, no markdown.
- Body explains *why*; include:
  - `Fixes: <12-sha> ("<subject>")` for bug fixes
  - `Assisted-by: LLM [tools]` on every agent-authored commit
  - NEVER `Signed-off-by:` from an agent — human integrator adds theirs.

## 3. Semantic spec (before any code)

`specs/<ID>.md`, committed with the port:

1. What the C code does — observable behavior, not line-by-line narration.
2. Locking inventory: every lock, what it protects, ordering constraints.
3. Error paths: every failure mode and its userspace-visible result.
4. UAPI surface touched (must be empty for phase 1–2).
5. Known C quirks preserved deliberately ("bug-compatible") with justification.

The Reviewer reviews spec-vs-C first; code is only reviewed against the spec.

## 4. Parity testing

For each ported driver, Test Engineer builds a C-kernel and a Rust-kernel
image and drives identical syscall sequences through both (scripted via the
fuzzing infrastructure). Observable results (return values, data read back,
sysfs contents) must match except where the spec documents deliberate change.

## 5. Honesty requirements (per coding-assistants.rst)

Every handoff/PR description states explicitly:
- What was built/tested here and with which exact commands
- What could NOT be built/tested/reproduced in this environment
- Known gaps vs the checklist (unchecked boxes are listed, not hidden)

## 6. Escalation matrix

| Situation | Escalate to |
|---|---|
| UAPI change needed | Human Integrator (hard block otherwise) |
| Spec/C behavior mismatch discovered mid-port | Orchestrator → freeze port |
| Found bug in C code outside port scope | File report per AGENTS.md: reproducer first, attempt fix same session, state untested parts |
| Two agents disagree on semantics | Orchestrator decides; recorded in TRACKER notes |
| Verification cannot be satisfied (Kani can't model X) | Verifier documents limitation; Reviewer + human decide risk |

## 7. Definition of done (a port row goes `ready` only when ALL true)

- [ ] Semantic spec approved
- [ ] Builds x86_64 + arm64, W=1 clean, clippy clean, rustfmt clean
- [ ] checkpatch --strict clean
- [ ] KUnit/doctests pass; property tests ≥10k cases pass
- [ ] Kani proofs green (or documented limitation + sign-off)
- [ ] Fuzz campaign ≥24h clean (driver ports)
- [ ] Safety audit: zero new uncommented `unsafe`
- [ ] Parity suite green
- [ ] Reviewer approval recorded in TRACKER
