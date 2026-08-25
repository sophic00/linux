# P-002 — Semantic spec: `dummy-irq` Rust port

Source: `drivers/misc/dummy-irq.c` (~60 lines, Jiri Kosina, 2013).
Target: `drivers/misc/rust_dummy_irq.rs`, module `rust_dummy_irq`,
Kconfig `RUST_DUMMY_IRQ` (tristate, `depends on RUST`).

## 1. Observable behavior of the C code

The driver is **not** a platform/busc device driver. It is a bare
`module_init`/`module_exit` module:

### Module parameter

- `module_param_hw(irq, uint, irq, 0444)` backed by `static int irq = -1;`
- Settable only at module load time (`insmod dummy_irq irq=N`). Permissions
  0444 make it world-readable but not writable via sysfs; the value used by
  the driver is fixed for the lifetime of the module load ("changing it after
  load has no effect until reload").
- The storage is an `int` but parsing uses `kstrtouint`: values up to
  `0xFFFFFFFF` are accepted at parse time; anything with bit 31 set makes the
  stored `int` negative and therefore hits the "no IRQ given" path below.

### Init (`dummy_irq_init`, runs at module load)

1. If `irq < 0` (including the `-1` default):
   `printk(KERN_ERR "dummy-irq: no IRQ given.  Use irq=N\n")`
   (note: two spaces after "given."), return `-EIO`. Module load fails.
2. Call `request_irq(irq, &dummy_interrupt, IRQF_SHARED, "dummy_irq", &irq)`.
   - The C code does **not** inspect or propagate the `request_irq` errno.
   - On failure: `printk(KERN_ERR "dummy-irq: cannot register IRQ %d\n", irq)`,
     return `-EIO`.
3. On success: `printk(KERN_INFO "dummy-irq: registered for IRQ %d\n", irq)`.

### IRQ handler (`dummy_interrupt`, hard-IRQ context)

- `static int count = 0;` — one counter per loaded module instance; resets on
  reload.
- On the first invocation: `printk(KERN_INFO "dummy-irq: interrupt occurred on
  IRQ %d\n", irq)` using the IRQ number as delivered to the handler (which for
  a shared line may be the line number requested), then `count++`.
- Every subsequent invocation prints nothing.
- Always returns `IRQ_NONE`, even for the interrupt it printed about (this is
  deliberate: the module exists to observe spurious IRQs without claiming them).

### Exit (`dummy_irq_exit`, runs at module unload)

1. `printk(KERN_INFO "dummy-irq unloaded\n")` — printed **before** freeing.
2. `free_irq(irq, &irq)` — cookie is the address of the parameter storage,
   matching `request_irq`.

If init failed, exit never runs (the module never loaded).

## 2. Locking inventory

None. The C driver takes no locks. The `static int count` in the handler is
unsynchronized (benign debug-driver race; worst case a duplicated message).
The Rust port uses an LKMM atomic xchg to guarantee exactly-once printing;
this is strictly stronger synchronization with identical observable behavior
(see §5).

## 3. Error paths

| Condition | Message (level) | Result |
|---|---|---|
| `irq < 0` at load | `dummy-irq: no IRQ given.  Use irq=N` (KERN_ERR) | init returns `-EIO`; insmod fails |
| `request_irq()` failed | `dummy-irq: cannot register IRQ %d` (KERN_ERR) | init returns `-EIO`; insmod fails |

Both paths deliberately return `-EIO` regardless of the underlying cause
(`-EINVAL`/`-EBUSY`/`-ENOMEM` from `request_irq` are swallowed). This quirk is
preserved.

## 4. UAPI surface

**Empty.** No ioctl/proc/sysfs/netlink interfaces beyond the standard module
parameter mechanism (see deviation D1 regarding sysfs visibility).

## 5. Deliberate C quirks preserved (bug-compatible)

1. **Errno swallowing**: any `request_irq` failure maps to `-EIO`, never the
   real errno.
2. **uint-into-int wrap**: parameter parsed as unsigned 32-bit; values ≥ 2³¹
   are accepted at parse time and treated as "no IRQ given" (`-EIO`) exactly
   as the C `int` storage would behave. Rust models this with a `u32`
   parameter and a `> i32::MAX` check rather than rejecting at parse time.
3. **Handler always returns `IRQ_NONE`**, even after logging.
4. **Exactly one message per module instance**, reset on reload.
5. **Cookie = address of parameter storage**: `&irq` is both `dev_id` at
   request and free time. The port passes the address of the generated
   parameter static for the same effect (the handler ignores the cookie).
6. **Exit prints before `free_irq`.**
7. **Two spaces** in `"no IRQ given.  Use irq=N"`.

## 6. Documented deviations from C (unavoidable / non-observable)

- **D1 — sysfs param visibility**: the Rust `module!` macro currently emits
  parameters with `perm = 0` ("will not appear in sysfs") and offers no way to
  specify permissions. The C param is 0444-readable in
  `/sys/module/<mod>/parameters/irq`. Since the C param is read-only anyway
  and cannot influence the running driver, this is cosmetic; noted here so
  parity testers don't chase it. Fixing requires `rust/macros/module.rs`
  changes (out of scope for P-002).
- **D2 — modinfo parmtype spelling**: `parmtype:irq:u32` vs C's
  `irq:uint`. Cosmetic.
- **D3 — count synchronization**: implemented with `Atomic<u32>::xchg`
  instead of a racy plain static; observable behavior identical
  (exactly-once print), race eliminated.

## 7. Implementation constraints honored

- No changes to `rust/kernel/` or UAPI headers.
- The existing `kernel::irq::Registration` abstraction cannot be used because
  it requires a bound `struct device` (`IrqRequest::new` is device-bound),
  while the C module registers a raw IRQ number at module init. The port calls
  `bindings::request_irq`/`bindings::free_irq` directly with full SAFETY
  justifications. A follow-up task may add a device-less raw registration API
  to `rust/kernel/irq/`.
