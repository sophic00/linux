# Fuzzing Strategy

Fuzzing is the only layer that finds *unknown unknowns* in mixed C/Rust code.
Two complementary setups:

## 1. System-call fuzzing (whole-kernel) — syzkaller

Coverage-guided syscall fuzzing against a QEMU boot of this tree with
sanitizers on. Template config: `syzkaller.cfg.example`.

Setup (runner host, not this workspace):

```sh
git clone https://github.com/google/syzkaller && cd syzkaller && make
# build kernel with: CONFIG_KCOV=y CONFIG_DEBUG_INFO=y CONFIG_KASAN=y \
#   CONFIG_KCSAN=y CONFIG_RUST=y CONFIG_SAMPLES_RUST=<your driver>
./bin/syz-manager -config=syzkaller.cfg.example
```

Rust drivers are fuzzed like C ones — via their userspace ABI. Anything a Rust
port changes semantically will surface here as crashes/diffs vs the C build.

## 2. In-process fuzzing (parser/logic crates) — libFuzzer

Pure logic modules (the same ones used for property tests) get libFuzzer
targets when registry access allows installing `cargo-fuzz`:

```sh
cargo install cargo-fuzz
cargo fuzz run parse_packet -- -max_total_time=86400 -dict=fuzz.dict
```

Corpus seeds come from the property-test generators; any crash must also be
reduced to a failing property or unit test before the fix lands.

## Harness checklist (per ported driver)

See `harness-checklist.md` — a port is not "testing" green without:
- [ ] ≥24h syzkaller uptime, KASAN+KCSAN enabled, zero new crash signatures
- [ ] All ioctls/sysfs/proc entries exercised by generated descriptions
- [ ] Error paths fuzzed (ENOSPC/EINTR/injected failures), not just happy path
- [ ] Crash triage log attached to the TRACKER row

## Triage rules

1. Reproduce first; unreproducible crashes get minimized via syz-repro.
2. Classify: Rust-introduced / pre-existing-C / harness bug.
3. Pre-existing C bugs: report per AGENTS.md procedure (verify → fix attempt
   → state what was not tested), even if outside the rewrite scope.
