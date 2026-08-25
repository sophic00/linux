# Linux Kernel → Rust Rewrite (scoped)

A **scoped, incremental** Rust rewrite of portable, self-contained components
of the Linux kernel found in this repository. This is not an attempt to
rewrite the whole kernel (~40M lines of C) — it is a working beachhead that
establishes the pattern: faithful semantics, ported kernel test suites, and
differential tests against reference implementations.

## Layout

```
rewrite/
├── Cargo.toml              # workspace
└── crates/
    ├── kstrtox/            # lib/kstrtox.c  — integer/boolean string parsing
    │   └── src/tests.rs    #   ported from lib/test-kstrtox.c
    └── ksort/              # lib/sort.c     — bottom-up heapsort
        │                   # lib/bsearch.c  — binary search
        └── src/tests.rs    #   incl. differential fuzz vs std::sort,
                            #   modeled on lib/tests/test_sort.c
```

## Component map

| Kernel source        | Rust crate       | Notes                                                        |
|----------------------|------------------|--------------------------------------------------------------|
| `lib/kstrtox.c`      | `kstrtox`        | Same autodetect/overflow/trailing-`\n` semantics; `Result` API replaces out-params and negative errnos |
| `lib/test-kstrtox.c` | `kstrtox::tests` | ok/fail tables ported                                        |
| `lib/sort.c`         | `ksort::heapsort_by` | Bottom-up heapsort; alignment-dispatch swap routines unnecessary in Rust |
| `lib/bsearch.c`      | `ksort::bsearch_by`  | Generic binary search                                    |
| `lib/tests/test_sort.c` | `ksort::tests`| Randomized differential testing vs std                        |

## Why these first

These subsystems are pure logic with no hardware, locking, or allocator
dependencies — exactly the class of kernel code where a safe-Rust rewrite can
be *provably* equivalent. They are also heavily used: `kstrtoull` has hundreds
of callers across `kernel/`, `fs/`, `drivers/`, and `sort()` underpins rbtree
and list operations.

## Build & test

```sh
cd rewrite
cargo test          # runs all crates' unit + differential tests
cargo clippy -- -D warnings
```

## Roadmap (next candidates in this repo)

1. `lib/base64.c`, `lib/glob.c`, `lib/ctype helpers` — more pure logic
2. `lib/rbtree.c`, `lib/list_sort.c` — intrusive containers become
   ownership-based generic containers (see also the kernel's own
   `rust/kernel/rbtree.rs`)
3. `lib/crc32.c`, `lib/xxhash.c` — checksums, cross-checked against the C
   test vectors
4. Parser layer: `lib/cmdline.c`, `vsprintf.c` format handling

Longer term, anything touching memory management, IRQs, or drivers must go
through the kernel's official Rust abstractions (`rust/kernel/`) rather than
this host-side workspace.

## License

GPL-2.0, matching the rewritten sources.
