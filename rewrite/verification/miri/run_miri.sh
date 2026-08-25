#!/usr/bin/env bash
# Miri runner for pure-Rust (no-FFI) logic crates.
# Miri detects UB: dangling pointers, alignment violations, data races (with
# -Zmiri-many-seeds), integer overflow paths that escape debug_asserts.
set -u
cd "$(dirname "$0")/../.." || exit 1   # rewrite/

if ! cargo +nightly miri --version >/dev/null 2>&1; then
    echo "Miri not installed. Install with:"
    echo "  rustup +nightly component add miri"
    exit 0
fi

# Many seeds: randomized exploration of scheduling/pointer nondeterminism.
MIRI_SEEDS="${MIRI_SEEDS:-64}"
EXTRA=()
if [[ "${1:-}" == "--race" ]]; then
    EXTRA=(-Zmiri-many-seeds="0..${MIRI_SEEDS}")
fi

for manifest in testing/property/*/Cargo.toml; do
    echo "=== Miri: $manifest ${EXTRA[*]:-} ==="
    # shellcheck disable=SC2086
    cargo +nightly miri test --manifest-path "$manifest" "${EXTRA[@]+"${EXTRA[@]}"}" \
        || { echo "MIRI FAILED on $manifest"; exit 1; }
done
echo "Miri clean."
