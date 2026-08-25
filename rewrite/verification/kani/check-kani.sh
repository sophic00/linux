#!/usr/bin/env bash
# Run Kani proofs for every host-test crate registered in this program.
set -u
cd "$(dirname "$0")/../.." || exit 1   # rewrite/

if ! cargo kani --version >/dev/null 2>&1; then
    echo "Kani not installed. On a runner host run:"
    echo "  cargo install --locked kani-verifier && cargo kani setup"
    exit 0   # degrade gracefully; CI marks SKIP via caller
fi

rc=0
for manifest in testing/property/*/Cargo.toml; do
    echo "=== Kani: $manifest ==="
    cargo kani --manifest-path "$manifest" || rc=1
done
exit $rc
