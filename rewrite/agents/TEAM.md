# Agent Team Structure

Human Integrator + six cooperating agents. Each role has a matching pi skill
(`.pi/skills/`) so any agent instance can be instantiated into a role.

## Roles

### Orchestrator
Owns `rewrite/TRACKER.md` and PLAN.md phases. Batches independent ports,
assigns Porter/Test/Verify roles, resolves cross-agent conflicts, maintains
the blockers log, prepares weekly status for the human.

### Porter (N instances — the parallel workforce)
Executes one port per TRACKER row: writes the semantic spec, does the C→Rust
translation, keeps kbuild green. **Does not** self-verify beyond compiling;
hands off at `porting → testing`. Skill: `/skill:kernel-port`.

### Test Engineer (per batch of ports)
Owns testing layers 1–2 for assigned ports: KUnit/doctests, property crates
(following `testing/property` pattern), parity harnesses. Blocks on coverage
gaps rather than marking green. Skill: `/skill:kernel-test-harness`.

### Verifier
Owns layers 3–5: Kani proofs for every new `unsafe`, Loom models for locking
deviations, fuzz campaign setup/triage. Signs the "verification complete"
box in TRACKER or documents why it's impossible. Skill: `/skill:kernel-verify`.

### Reviewer (independent of Porter — never same agent both sides)
Reviews spec-vs-C first, then code-vs-spec. Checks: unsafe hygiene, error-path
equivalence, lock ordering preservation, UAPI freeze, checkpatch/clippy/fmt.
Runs `ci/check.sh all` locally before approving. Skill: `/skill:kernel-review`.

### Human Integrator (the only human in the loop)
Final authority: trusts projects, reviews `ready` rows, adds `Signed-off-by`
(the only DCO signature), sends patches, arbitrates escalations that
PROTOCOLS.md §6 cannot resolve.

## Interaction rules

1. Handoffs happen through TRACKER.md status changes only — no side channels.
2. Reviewer must be a *different* agent session than the Porter of that code.
3. Any agent finding a bug outside its scope follows AGENTS.md bug procedure:
   reproduce → attempt fix same session → state explicitly what was untested.
4. Agents never merge their own work; `ready → submitted` is human-only.
