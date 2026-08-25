# Checklist: Reviewer Protocol

You review in this exact order. Stop and reject at the first failed stage —
reviewing code against a broken spec wastes everyone's tokens.

## Stage 1 — Spec vs C (before reading ANY Rust)
- [ ] Re-read the C driver independently; diff against specs/<ID>.md
- [ ] Locking inventory accurate? Ordering constraints captured?
- [ ] Error paths complete? errno mapping correct?
- [ ] Any observable behavior missing from spec?

## Stage 2 — Code vs Spec
- [ ] Every spec behavior has a corresponding Rust path
- [ ] No extra behaviors not in spec (invention is a defect)
- [ ] Lock acquisition/release matches C scheme 1:1 (or documented deviation w/ Verifier sign-off)
- [ ] IRQ/workqueue context respected (spinlocks vs mutexes identical to C)
- [ ] Drop/impl paths can't sleep while holding spinlocks

## Stage 3 — Hygiene gates (run them, don't trust claims)
```sh
cd rewrite && ci/check.sh fmt lint audit
./scripts/checkpatch.pl --strict <patches>
```
- [ ] All green; zero new uncommented unsafe
- [ ] No UAPI diffs (`git diff --stat -- include/uapi rust/uapi` empty)
- [ ] Commits: subject format, Assisted-by present, NO Signed-off-by by agent

## Stage 4 — Testing artifacts
- [ ] KUnit/doctest results attached; property tests exist and pass
- [ ] Kani proofs reviewed for vacuity (assumptions don't trivialize assertions)
- [ ] Fuzz campaign log ≥24h clean (driver ports only)

## Verdict
- Approve → TRACKER status `ready`, approval noted with date
- Reject → specific, actionable notes in TRACKER notes column, status back one stage
