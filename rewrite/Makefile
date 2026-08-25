# Convenience wrapper around ci/check.sh stages.
# Usage: make -C rewrite <target>

SHELL := /bin/bash
CI := ci/check.sh

.PHONY: help check fmt lint build-x86 build-arm64 docs unit audit \
        property kunit miri safety verify-all

help:
	@echo "Rewrite program targets:"
	@echo "  fmt         - rustfmt gate (fast)"
	@echo "  lint        - clippy gate"
	@echo "  build-x86   - x86_64 defconfig build (out-of-tree)"
	@echo "  build-arm64 - arm64 LLVM build (out-of-tree)"
	@echo "  docs        - rustdoc gate"
	@echo "  unit        - rusttest (host doctests)"
	@echo "  kunit       - KUnit Rust suites (needs QEMU)"
	@echo "  property    - host-side property tests"
	@echo "  miri        - Miri UB detection (if installed)"
	@echo "  audit       - unsafe-block SAFETY-comment audit"
	@echo "  check       - full gate matrix"

check: ;	@$(CI) all
fmt: ;		@$(CI) fmt
lint: ;		@$(CI) lint
build-x86: ;	@$(CI) build-x86
build-arm64: ;	@$(CI) build-arm64
docs: ;		@$(CI) docs
unit: ;		@$(CI) unit
audit: ;	@$(CI) audit
property: ;	cargo test --manifest-path testing/property/kernel-logic-hosttests/Cargo.toml
kunit: ;	python3 ../tools/testing/kunit/kunit.py run --kunitconfig testing/kunit/rust-extended.kunitconfig
miri: ;		./verification/miri/run_miri.sh
safety: audit
verify-all: property audit
	@./verification/kani/check-kani.sh || true
	@./verification/miri/run_miri.sh || true
