# P-005 — Semantic spec: `hangcheck-timer` Rust port

Source: `drivers/char/hangcheck-timer.c` (~150 lines, Oracle, 2002–2003,
v0.9.1).
Target: `drivers/char/rust_hangcheck_timer.rs`, module `hangcheck_timer`
(name collision with C module name is impossible to have loaded
simultaneously; the C one registers `"hangcheck-timer"` via its Makefile
object while `MODULE_NAME` derives from the object name; see §7 D6), Kconfig
`RUST_HANGCHECK_TIMER` (tristate).

## 0. Scope note — task description vs. in-tree C source

The port assignment text mentions a `hangcheck_start_timer` parameter, an
"hrtimer-based" loop, and a "monotonic-vs-TSC duration check". **None of
these exist in the in-tree C source**, which is the single source of truth
per PROTOCOLS §3:

- The current driver reads `ktime_get_ns()` (CLOCK_MONOTONIC) only. There is
  no raw-TSC read and no arch-specific code anymore (the historical x86/
  PPC/S390 TSC plumbing was removed upstream long ago; the Kconfig arch
  restriction is a fossil).
- The firing loop uses a classic `timer_list` (`DEFINE_TIMER` +
  `mod_timer()` + jiffy re-arm), **not** an hrtimer, despite the vestigial
  `#include <linux/hrtimer.h>`.
- There is no `hangcheck_start_timer` parameter; init unconditionally starts
  the timer.

This spec ports what the C code does today. Divergences between the task
text and the source are recorded here rather than "restoring" behavior the
code no longer has.

## 1. Observable behavior of the C code

Bare `module_init`/`module_exit` char-driver-style module. No device node,
no file operations, no IRQ, no workqueue.

### Module parameters (all `int`, permission 0)

| Param                  | Default | Meaning                                   |
|------------------------|---------|-------------------------------------------|
| `hangcheck_tick`       | 180     | Timer period in seconds                    |
| `hangcheck_margin`     | 60      | Allowed overshoot in seconds               |
| `hangcheck_reboot`     | 0       | If nonzero, reboot on margin exceed        |
| `hangcheck_dump_tasks` | 0       | If nonzero, SysRQ-t dump on margin exceed  |

Note the header comment ("defaults are 60 seconds for the timer and 180
seconds for the margin") contradicts the defines
(`DEFAULT_IOFENCE_TICK 180`, `DEFAULT_IOFENCE_MARGIN 60`) — the defines win;
tick=180, margin=60. Quirk preserved (§5 Q1).

Permission 0 means: settable only at load/boot time, invisible in sysfs.
When built-in (`!MODULE`), four legacy `__setup` boot options exist:
`hcheck_tick=`, `hcheck_margin=`, `hcheck_reboot=`, `hcheck_dump_tasks=`.
These parse with `get_option()` and silently ignore malformed values.

### Init (`hangcheck_init`, never fails)

1. `pr_debug("Hangcheck: starting hangcheck timer %s (tick is %d seconds,
   margin is %d seconds).\n", VERSION_STR, tick, margin)` with
   VERSION_STR = "0.9.1".
2. `hangcheck_tsc_margin = ((unsigned long long)hangcheck_margin +
   hangcheck_tick) * TIMER_FREQ` where TIMER_FREQ = 1000000000ULL. Note the
   cast happens **before** the addition: negative ints wrap as u64.
3. `hangcheck_tsc = ktime_get_ns()`.
4. Arm the timer: `mod_timer(&hangcheck_ticktock, jiffies +
   hangcheck_tick*HZ)` — first expiry one tick-period after init.
5. Return 0. **Init cannot fail**; there is no error path.

### Timer callback (`hangcheck_fire`, soft-timer context)

1. `cur_tsc = ktime_get_ns()`.
2. `tsc_diff = cur_tsc > hangcheck_tsc ? cur_tsc - hangcheck_tsc
   : cur_tsc + (~0ULL - hangcheck_tsc)` — u64 wraparound arithmetic over an
   unsigned counter (defensive; CLOCK_MONOTONIC nanoseconds will not wrap in
   practice). The `else` branch computes `diff = cur - last - 1` in modular
   terms... precisely: `cur + (!last)` where `~last == !0ULL - last`, i.e.
   modular subtraction assuming wrap past 2^64-1.
3. If `tsc_diff > hangcheck_tsc_margin`:
   - If `hangcheck_dump_tasks`: `pr_crit("Hangcheck: Task state:\n")` then,
     under `CONFIG_MAGIC_SYSRQ`, `handle_sysrq('t')`.
   - If `hangcheck_reboot`: `pr_crit("Hangcheck: hangcheck is restarting the
     machine.\n")` then `emergency_restart()` (never returns).
   - Else: `pr_crit("Hangcheck: hangcheck value past margin!\n")`.
4. Re-arm: `mod_timer(&hangcheck_ticktock, jiffies + hangcheck_tick*HZ)`
   — next expiry measured from the moment of re-arm.
5. `hangcheck_tsc = ktime_get_ns()` — baseline sampled **after** re-arming,
   so the measured interval spans slightly more than one tick period (the
   gap between arming and sampling plus scheduling latency of the callback
   start).

A `#if 0` debug print exists; dead code, not ported.

### Exit (`hangcheck_exit`)

1. `timer_delete_sync(&hangcheck_ticktock)` — waits for a running callback.
2. `pr_debug("Hangcheck: Stopped hangcheck timer.\n")`.

## 2. Locking inventory

**None.** The C driver takes no locks. Shared mutable state:

- `hangcheck_tsc`, `hangcheck_tsc_margin`: plain globals. Only the single
  timer callback touches them after init; a `timer_list` callback cannot run
  concurrently with itself, so no lock is needed. Init writes happen before
  the timer can fire; exit's `timer_delete_sync` provides the barrier.
- Module params: perm 0 ⇒ fixed after load.

Rust port keeps this shape: state lives in one `Arc<HangcheckState>`; the
baseline timestamp is an `AtomicU64` (Relaxed) purely to satisfy the borrow
checker for the callback context — LKMM-relaxed matches C's unsynchronized
access semantics for a self-serialized callback.

## 3. Error paths

Exactly one: **none**. `hangcheck_init` always returns 0. Parameter parsing
failures are handled by the generic module-param machinery (load fails with
-EINVAL if a supplied value doesn't parse — same as C `module_param`).
`emergency_restart()` does not return.

## 4. UAPI surface touched

**Empty.** No proc/sysfs/dev interfaces beyond the standard module-parameter
mechanism (perm 0 ⇒ no sysfs exposure, matching C).

## 5. Deliberate C quirks preserved (bug-compatible)

1. **Wrong header-comment defaults**: comment says 60/180, code says
   tick=180 margin=60. Code wins.
2. **u64 wraparound diff formula**: preserved verbatim semantics using
   wrapping u64 ops (`cur.wrapping_add(!last)` in the else branch).
3. **Margin computed with pre-cast addition**: `(margin as u64)
   .wrapping_add(tick as u64) * 1_000_000_000` — negative params produce the
   identical wrapped u64 threshold as C.
4. **Baseline sampled after re-arm decision**: see §7 D2 for how this maps
   onto hrtimer restart ordering.
5. **`handle_sysrq('t')` compiled out without CONFIG_MAGIC_SYSRQ**, message
   still printed (cfg-gated call, unconditional pr_crit).
6. **No upper bound checks on params**: huge/negative tick/margin accepted;
   negative tick wraps in jiffy arithmetic in C (UB-ish but benign); in the
   port a non-positive tick clamps the hrtimer delta to ≥0 (see §7 D4).

## 6. Deliberate deviations forced by abstraction gaps

D1. **hrtimer instead of `timer_list`.** The Rust kernel crate exposes only
    `HrTimer` (rust/kernel/time/hrtimer.rs); no classic-timer abstraction
    exists. Behavior deltas: sub-jiffy resolution (finer than C, harmless —
    C's jiffy granularity only ever makes the check *less* prompt), and
    re-arm uses `HrTimerCallbackContext::forward_now(Delta)` +
    `HrTimerRestart::Restart`, equivalent to `mod_timer(now + tick)`.
    Clock: `RelativeMode<Monotonic>` = same `ktime_get()` source the C code
    samples manually.

D2. **Baseline sample point.** C samples the new baseline *after* calling
    `mod_timer`; with hrtimers the re-arm completes after the callback
    returns, so the port samples the new baseline immediately before
    returning Restart. The difference is a few microseconds of extra
    measured headroom per cycle — strictly widens the observed interval by
    less than scheduler noise; margin logic unchanged.

D3. **No `__setup("hcheck_*")` boot options when built-in.** The Rust
    module-param macro has no `__setup` equivalent; built-in users must use
    the standard `hangcheck_timer.hangcheck_tick=N` built-in param cmdline
    form instead of `hcheck_tick=N`. Documented userspace-visible change for
    builtin builds only; module builds unaffected.

D4. **Tick/margin ≤ 0.** In C, `jiffies + tick*HZ` with negative tick arms a
    timer in the past → fires ~immediately, repeatedly. With
    `Delta::from_seconds(neg)` an hrtimer refuses/behaves differently; the
    port converts the tick once at init with `max(0, tick)` semantics and
    documents it. Negative margins still reproduce C's wrapped-u64
    threshold exactly (Q3).

D5. **No `MODULE_VERSION`.** The Rust `module!` macro has no `version:`
    field yet; the "0.9.1" version string is kept in the crate docs instead
    of modinfo. (Macro extension rejected: out-of-scope rust/kernel change.)

D6. **Module name.** Rust module name is `hangcheck_timer` (underscores are
    mandatory in the macro); C object/module is `hangcheck-timer`. Same
    visible naming convention as other Rust ports.

## 7. Kconfig / build wiring

```
config RUST_HANGCHECK_TIMER
	tristate "Hangcheck timer (Rust)"
	depends on RUST
	help ...
```

Deviation D7: the C symbol depends on `X86 || PPC64 || S390`. That
restriction dates from raw-TSC days; the current C code (and hence the port)
uses only generic `ktime_get_ns()`, which works everywhere. Keeping the
arch restriction would make the required arm64 verification build
impossible, so the port drops it. A human reviewer may reinstate
`X86 || PPC64 || S390` if strict config parity is preferred; runtime
behavior on the three supported arches is unaffected either way.
