#!/usr/bin/env bash
# Full gate matrix for the Rust rewrite program.
# Every stage degrades gracefully when its tooling is missing; the summary at
# the end reports PASS/SKIP/FAIL. CI should treat SKIP as "fix your runner".
set -u
cd "$(dirname "$0")/.." || exit 1   # rewrite/
KDIR="$(pwd)/.."                    # kernel tree root
STAGES=("${@:-all}")
[[ "${STAGES[0]}" == "all" ]] && STAGES=(fmt lint build-x86 build-arm64 docs unit audit)

declare -A RESULT
O=/tmp/rewrite-build-$$

run() { # run <name> <cmd...>
    local name="$1"; shift
    echo "=== [$name] $* ==="
    if "$@" >"/tmp/rw-$name.log" 2>&1; then
        RESULT[$name]=PASS; tail -3 "/tmp/rw-$name.log"
    else
        RESULT[$name]=FAIL; echo "FAILED — see /tmp/rw-$name.log (tail):"; tail -20 "/tmp/rw-$name.log"
    fi
}

skip() { RESULT[$1]="SKIP ($2)"; }

want() { local s; for s in "${STAGES[@]}"; do [[ "$s" == "$1" ]] && return 0; done; return 1; }

# ---- fmt -------------------------------------------------------------------
if want fmt; then
    if command -v rustfmt >/dev/null; then
        run fmt make -C "$KDIR" LLVM=1 O="$O-x86" rustfmtcheck
    else skip fmt "rustfmt missing"; fi
fi

# ---- lint ------------------------------------------------------------------
if want lint; then
    if cargo clippy --version >/dev/null 2>&1; then
        run lint env CLIPPY=1 make -C "$KDIR" LLVM=1 WERROR=1 O="$O-x86" prepare  # configure
        run lint2 env CLIPPY=1 make -C "$KDIR" LLVM=1 WERROR=1 O="$O-x86" -j"$(nproc)"
        [[ "${RESULT[lint2]}" == FAIL ]] && RESULT[lint]=FAIL
    else skip lint "clippy missing"; fi
fi

# ---- builds ----------------------------------------------------------------
if want build-x86; then
    run build-x86 make -C "$KDIR" LLVM=1 defconfig O="$O-x86"
    run build-x86 make -C "$KDIR" LLVM=1 -j"$(nproc)" O="$O-x86"
fi

if want build-arm64; then
    if rustup target list --installed 2>/dev/null | grep -q aarch64; then
        run build-arm64 make -C "$KDIR" LLVM=1 ARCH=arm64 defconfig O="$O-arm64"
        run build-arm64 make -C "$KDIR" LLVM=1 ARCH=arm64 -j"$(nproc)" O="$O-arm64"
    else skip build-arm64 "aarch64 rust target not installed"; fi
fi

# ---- docs / unit -----------------------------------------------------------
if want docs; then
    command -v rustdoc >/dev/null && run docs make -C "$KDIR" LLVM=1 O="$O-x86" rustdoc \
        || skip docs "rustdoc missing"
fi
if want unit; then
    run unit make -C "$KDIR" LLVM=1 O="$O-x86" rusttest
fi

# ---- audit -----------------------------------------------------------------
if want audit; then
    if python3 ci/safety-audit.py --baseline ci/unsafe-baseline.txt >/tmp/rw-audit.log 2>&1; then
        RESULT[audit]=PASS; tail -3 /tmp/rw-audit.log
    else
        RESULT[audit]="FAIL (new uncommented unsafe)"; tail -20 /tmp/rw-audit.log
    fi
fi

# ---- summary ---------------------------------------------------------------
echo; echo "======== GATE SUMMARY ========"
fail=0
for k in fmt lint build-x86 build-arm64 docs unit audit; do
    r="${RESULT[$k]:-not-run}"
    printf '  %-12s %s\n' "$k" "$r"
    [[ "$r" == FAIL* ]] && fail=1
done
echo "=============================="
rm -rf "$O"-x86 "$O"-arm64
exit $fail
