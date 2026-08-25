#!/usr/bin/env bash
# Long-running in-process differential fuzz loops.
#
# Discovers every crates/*/src/bin/fuzz-*.rs binary and runs each for
# FUZZ_SECONDS (default 20) with a rotating seed. Deterministic seeds can be
# forced via FUZZ_SEED=<n>. Exits nonzero if any binary fails.
#
# Usage: scripts/fuzz.sh [SECONDS_PER_BINARY]
set -euo pipefail
cd "$(dirname "$0")/.."

FUZZ_SECONDS="${1:-${FUZZ_SECONDS:-20}}"
counter=0
failures=0

for bin_src in crates/*/src/bin/fuzz-*.rs; do
    [ -e "$bin_src" ] || continue   # glob unmatched -> nothing to do yet
    pkg=$(basename "$(dirname "$(dirname "$(dirname "$bin_src")")")")
    name=$(basename "$bin_src" .rs)
    counter=$((counter + 1))

    if [ -n "${FUZZ_SEED:-}" ]; then
        seed="$FUZZ_SEED"
    else
        # Rotating seed: wall clock mixed with a stable hash of the binary
        # name, so concurrent CI jobs on different binaries diverge while
        # FUZZ_SEED still allows exact replay.
        seed=$(( ($(date +%s) ^ $(printf '%s' "$name" | cksum | cut -d' ' -f1) ^ counter) & 0x7fffffff ))
    fi

    echo "== fuzz $pkg/$name (${FUZZ_SECONDS}s, seed=$seed)"
    if ! cargo run -q -p "$pkg" --bin "$name" -- "$FUZZ_SECONDS" "$seed"; then
        echo "!! fuzz target FAILED: $pkg/$name"
        failures=$((failures + 1))
    fi
done

if [ "$counter" -eq 0 ]; then
    echo "no fuzz-* binaries found; nothing to do"
    exit 0
fi

echo "fuzz summary: $counter target(s), $failures failure(s)"
exit "$failures"
