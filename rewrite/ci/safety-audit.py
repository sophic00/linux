#!/usr/bin/env python3
"""Audit `unsafe` blocks in Rust kernel code for SAFETY comments.

Kernel convention: every `unsafe` block/fn/trait impl carries a `// SAFETY:`
comment explaining why the operation is sound. This script enforces it and,
with --baseline, fails only on NEW violations so legacy debt doesn't block CI.

Usage:
  safety-audit.py [--baseline FILE] [--paths PATH ...] [--report-only]

Exit codes: 0 = no new violations, 1 = new violations found.
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path

SAFETY_RE = re.compile(r"//\s*SAFETY:", re.IGNORECASE)
UNSAFE_RE = re.compile(r"\bunsafe\b")
DEFAULT_PATHS = ["rust", "drivers", "samples"]

# Anchor on script location, not CWD: rewrite/ci/safety-audit.py
REWRITE_DIR = Path(__file__).resolve().parent.parent
KERNEL_ROOT = REWRITE_DIR.parent


def tracked_rust_files(paths):
    files = []
    git = subprocess.run(
        ["git", "ls-files", "--", "*.rs", *[f"{p}/**/*.rs" for p in paths]],
        capture_output=True, text=True, cwd=KERNEL_ROOT)
    for line in git.stdout.splitlines():
        p = KERNEL_ROOT / line.strip()
        if p.exists():
            files.append(p)
    return sorted(set(files))


def violations_in(path):
    """Return line numbers of unsafe blocks lacking a preceding SAFETY comment."""
    out = []
    try:
        lines = path.read_text(errors="replace").splitlines()
    except OSError:
        return out
    for i, line in enumerate(lines):
        stripped = line.strip()
        # Heuristic: statement-level unsafe blocks (`unsafe {`), not signatures.
        if re.search(r"\bunsafe\s*\{", line) is None:
            continue
        window = "\n".join(lines[max(0, i - 5):i])
        if not SAFETY_RE.search(window) and "// SAFETY:" not in line:
            out.append(i + 1)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--baseline", help="file with known violation counts per file")
    ap.add_argument("--paths", nargs="*", default=DEFAULT_PATHS)
    ap.add_argument("--report-only", action="store_true")
    args = ap.parse_args()

    baseline = {}
    if args.baseline and Path(args.baseline).exists():
        for ln in Path(args.baseline).read_text().splitlines():
            if ln.strip():
                name, _, count = ln.rpartition(":")
                baseline[name] = int(count)

    total_new = 0
    report = []
    for f in tracked_rust_files(args.paths):
        v = violations_in(f)
        key = str(f.relative_to(KERNEL_ROOT))
        known = baseline.get(key)
        new = [l for l in v if known is None or len(v) > known]
        if v:
            report.append((key, len(v), len(new), v[:8]))
            total_new += max(0, len(v) - (known if known is not None else len(v)))

    for key, tot, new, lines in report:
        mark = "NEW" if new else "ok "
        print(f"[{mark}] {key}: {tot} uncommented unsafe ({new} new) e.g. lines {lines}")

    print(f"\nFiles audited with violations: {len(report)}; new violations: {total_new}")
    if args.report_only or args.baseline is None:
        print("report-only mode (pass --baseline to enforce)")
        return 0
    return 1 if total_new else 0


if __name__ == "__main__":
    sys.exit(main())
