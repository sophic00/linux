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
echo "OK: all gates green"
