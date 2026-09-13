#!/usr/bin/env python3
"""Fail CI when production/runtime Rust code contains explicit implementation placeholders.

This check deliberately ignores tests, benches, examples and validation tooling. It is not a
substitute for code review; it prevents known placeholder markers from silently entering the
runtime surface.
"""

from __future__ import annotations

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
CRATES = ROOT / "crates"

MARKERS = (
    re.compile(r"\btodo!\s*\("),
    re.compile(r"\bunimplemented!\s*\("),
    re.compile(r"\bplaceholder\b", re.IGNORECASE),
)

# Explicitly non-runtime source locations.
SKIP_PARTS = {"tests", "test", "benches", "examples"}


def iter_runtime_rust_files():
    for path in CRATES.rglob("*.rs"):
        rel = path.relative_to(ROOT)
        if any(part in SKIP_PARTS for part in rel.parts):
            continue
        if "src" not in rel.parts:
            continue
        yield path


def main() -> int:
    failures: list[str] = []
    for path in iter_runtime_rust_files():
        text = path.read_text(encoding="utf-8")
        for line_no, line in enumerate(text.splitlines(), 1):
            for marker in MARKERS:
                if marker.search(line):
                    failures.append(f"{path.relative_to(ROOT)}:{line_no}: {line.strip()}")
                    break

    if failures:
        print("Runtime placeholder audit FAILED:")
        for failure in failures:
            print(f"  {failure}")
        return 1

    print("Runtime placeholder audit PASS: no todo!/unimplemented!/placeholder markers in runtime Rust sources")
    return 0


if __name__ == "__main__":
    sys.exit(main())
