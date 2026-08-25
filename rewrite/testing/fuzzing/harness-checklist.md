# Per-Port Fuzz Harness Checklist

Driver: ____________  Port ID (TRACKER): ______  Agent: ______

## Surface enumeration (before any fuzzing)
- [ ] All char device majors/minors, misc devices registered
- [ ] Every ioctl number + direction (_IOC_DIR) documented in sysrus descriptions
- [ ] sysfs/procfs/debugfs attributes enumerated
- [ ] netlink families / socket options if any
- [ ] Module parameter space covered

## Coverage
- [ ] CONFIG_KCOV=y verified in test kernel
- [ ] /sys/kernel/debug/kcov reachable from fuzzer VM
- [ ] Coverage of Rust driver code confirmed (not just C helpers):
      `syz-cover` report shows lines from the .rs file

## Sanitizer matrix on the fuzz kernel
- [ ] KASAN (memory)   — CONFIG_KASAN=y
- [ ] KCSAN (races)    — CONFIG_KCSAN=y
- [ ] KFENCE (slab)    — CONFIG_KFENCE=y
- [ ] lockdep          — CONFIG_PROVE_LOCKING=y
- [ ] Rust overflow checks — CONFIG_RUST_OVERFLOW_CHECKS=y

## Campaign
- [ ] ≥24 hours wall time across ≥4 VMs
- [ ] Error-injection enabled (CONFIG_FAULT_INJECTION=y) and described
- [ ] Zero new crash signatures vs C-driver baseline run
- [ ] Behavioral parity spot-checks pass (same syscall sequence → same observable results C vs Rust)

## Sign-off
- [ ] Triage log path: ____________
- [ ] Crashes filed with Fixes:/report drafts where applicable
