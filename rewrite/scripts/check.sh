#!/usr/bin/env bash
# Full quality gate for the Rust rewrite workspace.
# Usage: scripts/check.sh          (all checks)
#        scripts/check.sh --fast   (skip fmt check)
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "${1:-}" != "--fast" ]]; then
    echo "== rustfmt =="
    cargo fmt --all -- --check
fi
echo "== clippy (-D warnings) =="
cargo clippy --workspace --all-targets -- -D warnings
echo "== tests =="
cargo test --workspace

# MERGE POLICY (rust-rewrite program): every crate must carry Kani proof
# harnesses in src/verify.rs (gated by --cfg kani so normal builds ignore it).
# This check enforces PRESENCE; scripts/verify.sh enforces the proofs pass.
echo "== verification-harness presence =="
missing=0
for d in crates/*/; do
    name=$(basename "$d")
    if [[ ! -f "$d/src/verify.rs" ]] || ! grep -q '#\[kani::proof\]' "$d/src/verify.rs" 2>/dev/null; then
        echo "FAIL: $name has no Kani harness (src/verify.rs with #[kani::proof])"
        missing=1
    fi
done
if [[ $missing -ne 0 ]]; then
    echo "OK requires: every crate provides panic-freedom + one spec-equivalence proof"
    exit 1
fi

echo "OK: all gates green"
