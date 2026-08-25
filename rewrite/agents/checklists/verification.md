# Checklist: Verification Engineer

Per TRACKER ID. You are the last line before "testing green" becomes a claim.

## Layer 1–2 sanity (Test Engineer handoff)
- [ ] Rerun `cargo test` on property crates yourself — never inherit results
- [ ] Confirm ≥10k cases per property; seeds recorded
- [ ] Doctests actually execute under KUnit config (not just compile)

## Layer 3 — Kani
- [ ] Every new `unsafe` block has a proof OR documented limitation + human ack
- [ ] Harnesses not vacuous: mutate an assertion locally → harness MUST fail
      (do this once per crate, revert after)
- [ ] Unwinds annotated with justification where used

## Layer 4 — Concurrency
- [ ] Locking identical to C → document; no model needed
- [ ] Any deviation → Loom model exists, passes, committed beside abstraction
- [ ] KCSAN-enabled boot log attached showing no reports over test workload

## Layer 5 — Fuzzing
- [ ] syzkaller descriptions cover full ioctl/attr surface (harness-checklist.md)
- [ ] ≥24h campaign, sanitizer matrix enabled, zero NEW signatures vs C baseline
- [ ] Crashes triaged: classification + reproducers archived

## Runtime visibility (mixed C/Rust requirement)
- [ ] KASAN reports Rust frames correctly (test with injected fault)
- [ ] lockdep sees Rust-held locks (deadlock smoke test)
- [ ] panic/oops output includes Rust symbolication

## Sign-off
- [ ] TRACKER layers column filled honestly; gaps listed in notes
- [ ] PROTOCOLS.md §5 honesty statement included in handoff message
