# P-001 — Semantic spec: Rust port of `drivers/misc/open-dice.c`

Source of truth: `drivers/misc/open-dice.c` at commit 502b6b9db74b (branch
base). Target: `drivers/misc/rust_open_dice.rs` (module `rust_open_dice`),
Kconfig `RUST_OPEN_DICE` (tristate, `depends on RUST && OF_RESERVED_MEM`).

## 1. Observable behavior

### Driver model
- Platform driver matching OF compatible `"google,open-dice"` (no ACPI id).
- On probe, takes ownership of a reserved-memory region referenced by the
  device's `of_node` via `of_reserved_mem_lookup()`. The kernel never
  interprets the contents.
- Registers one misc character device per probed instance, named
  `open-dice<idx>` where `idx` is a monotonically increasing counter starting
  at 0, shared across all probes (`static unsigned int dev_idx`). The counter
  increments even if registration subsequently fails (numbering gaps are
  observable and preserved).
- Device node mode: C driver sets `.mode = 0600`. The Rust miscdevice
  abstraction leaves `mode = 0`; the misc class default for nodes without an
  explicit mode is also 0600, so `/dev/open-dice<idx>` permissions are
  identical (crw-------).
- Minor: dynamic (`MISC_DYNAMIC_MINOR`) in both.
- Module init succeeds even with zero device instances (DICE regions are
  optional).

### read(2)
Returns the reserved-memory region size as a native-endian `unsigned long`
(8 bytes on arm64/x86_64), using `simple_read_from_buffer()` semantics:
honors `*ppos`, supports partial reads, returns bytes copied; reads from
`*ppos >= sizeof(unsigned long)` return 0; faults yield `-EFAULT` only after
partial copy accounting exactly as the shared helper implements. The size is
fixed after probe.

### write(2)
Any write, regardless of length or buffer content, triggers a wipe of the
reserved memory (see below) and consumes the whole buffer: returns `len`
including `len == 0`. The user pointer is never dereferenced in either
implementation (a NULL pointer with len 0 must not fault). If the wipe fails,
returns `-EIO`.

Wipe operation (serialized by the instance mutex):
1. map the rmem `[base, base+size)` write-combined into kernel VA,
2. memset it to zero,
3. unmap.

### mmap(2)
- If `VM_MAYSHARE` is set:
  - mapping with `VM_WRITE` set → `-EPERM`;
  - otherwise `VM_MAYWRITE` is cleared so userspace cannot gain writability
    via `mprotect()` later.
- Page protection is switched to write-combine (`pgprot_writecombine`) so that
  all clients observe wipes without explicit synchronization.
- `VM_DONTCOPY | VM_DONTDUMP` are set.
- The VMA is mapped ioremap-style to `[rmem->base, rmem->size)` honoring
  `vm_pgoff` (C: `mmap_action_simple_ioremap`; classic-API equivalent used by
  the port: `vm_iomap_memory()`, which performs the same
  `__simple_ioremap_prep` validation and `io_remap_pfn_range()` mapping, and
  sets `VM_PFNMAP|VM_IO|VM_DONTEXPAND` as the prepare path does).
- Mapping size/offset validation errors surface as the errors returned by the
  shared mm code (`-EINVAL` for pgoff/size mismatch, mapping failures as
  returned by `io_remap_pfn_range`).

## 2. Locking inventory

| Lock (C) | Lock (Rust) | Protects | Notes |
|---|---|---|---|
| `drvdata->lock` (`struct mutex`) | `Mutex<()>` inside per-instance `Arc<WipeState>` | the wipe critical section: memremap → memset → memunmap | Leaf lock; no ordering constraints; held across sleeping allocations exactly as in C |

No other locks are taken or ordered by this driver. Read/mmap paths do not
take the lock in either implementation.

## 3. Error paths and userspace-visible results

| Site | Condition | Result |
|---|---|---|
| probe | `of_reserved_mem_lookup(dev->of_node)` fails (incl. no OF node / non-OF fwnode) | `dev_err("failed to lookup reserved memory")`, probe fails with `-EINVAL` |
| probe | `rmem->size == 0` or `> ULONG_MAX` | `dev_err("invalid memory region size")`, `-EINVAL` |
| probe | `rmem->base` or `rmem->size` not page-aligned | `dev_err("memory region must be page-aligned")`, `-EINVAL` |
| probe | allocation failure | `-ENOMEM` |
| probe | `misc_register()` failure (e.g. duplicate name) | probe fails with the misc_register errno (typically `-EBUSY`) |
| open | `generic_file_open` failure | errno passthrough |
| read | user copy fault | `simple_read_from_buffer` result (`-EFAULT` if nothing copied) |
| write | wipe failure (memremap fails) | `-EIO` |
| mmap | `VM_MAYSHARE && VM_WRITE` | `-EPERM` |
| mmap | bad size/pgoff vs rmem range | `-EINVAL` (from `__simple_ioremap_prep`) |
| mmap | PTE population failure | errno from remap path |
| init | platform registration error | module init fails with that errno; `-ENODEV` (zero instances) maps to success |

## 4. UAPI surface touched

**Empty.** No changes under `include/uapi/` or `rust/uapi/`. The syscall-level
behavior (device names, modes, read/write/mmap semantics) is preserved as
specified above.

## 5. Deliberate quirks preserved ("bug-compatible")

1. **Device numbering gaps**: the instance index is consumed before
   `misc_register()` can fail; failed probes still burn an index.
2. **Write ignores the buffer entirely** — no `access_ok`, no copy; `len == 0`
   still wipes and returns 0.
3. **Read position semantics** — arbitrary `*ppos` values produce the exact
   byte windows defined by `simple_read_from_buffer`, including reads that
   straddle the 8-byte value.
4. **No fd-vs-unbind protection in C**: C frees drvdata via devm at remove
   time while open file descriptors may still exist (UAF hazard inherited from
   the original design). The Rust port *improves* on this by construction: each
   open file holds an `Arc` clone of the immutable wipe state (base, size,
   lock), so post-unbind use is memory-safe instead of a UAF. Syscall results
   are unchanged; only the latent crash-on-unbind-with-open-fd behavior
   differs.

## 6. Known deviations from the C source (all reviewed, none syscall-visible)

1. **mmap API generation**: C uses the new `mmap_prepare`/`vma_desc` API; the
   Rust `MiscDevice` abstraction exposes the classic `fops->mmap` hook
   (`VmaNew`). Equivalence is achieved by performing the same flag checks on
   `vm_flags` (which are final by the time the classic hook runs),
   `try_clear_maywrite()` for the MAYWRITE clear, and calling
   `vm_iomap_memory()` — the documented classic counterpart of
   `mmap_action_simple_ioremap()` (same `__simple_ioremap_prep` validation,
   same `io_remap_pfn_range()` mapping, same auto-set `VM_PFNMAP/VM_IO/
   VM_DONTEXPAND` flags).
   **Abstraction gap:** the Rust `VmaNew` API does not expose `page_prot`
   mutation, so the write-combine switch writes `vma->vm_page_prot` directly
   through the raw pointer (single `unsafe`, SAFETY-commented). This is the
   only place the port touches VMA internals outside the safe abstraction.
2. **Wipe uses `memremap()/memunmap()` instead of
   `devm_memremap()/devm_memunmap()`**: the C devres entry exists only between
   map and unmap within the same locked section, so there is no userspace-
   visible difference; dropping the device association also removes any need
   to pin a device reference in file-private data.
3. **Init uses plain platform driver registration**
   (`module_platform_driver!`) rather than `platform_driver_probe()`: with
   zero instances, registration simply succeeds (same observable init result
   as C's `-ENODEV → 0` mapping); unlike C, the driver stays registered and
   could bind devices appearing later — not reachable via DT boot flows that
   define DICE regions statically.
4. **Driver name**: C names the platform driver `open-dice`; the Rust
   registration ties the driver name to the module name
   (`/sys/bus/platform/drivers/rust_open_dice`). Device node names
   (`open-dice<idx>`) are unchanged.
5. **Name storage**: the dynamically formatted `open-dice<idx>` name is leaked
   (bounded by the number of successfully bound instances, ≤ a few) because
   `MiscDeviceOptions` requires a `&'static CStr`; C stores it in drvdata and
   frees it on remove.
6. **misc_register failure message**: the C driver prints
   `"failed to register misc device '%s': %d"`; the Rust miscdevice
   abstraction propagates the errno without a driver-side message. Errno
   semantics unchanged.
