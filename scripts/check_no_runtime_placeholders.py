#!/usr/bin/env python3
"""Reject executable Rust runtime stubs while allowing explicit control-plane schema vocabulary.

`todo!` and `unimplemented!` are forbidden in every production `src/` path. The word
`placeholder` is also forbidden except in the capability registry/presenter, where it is retained
only as a backwards-compatible schema field/enum name and does not construct executable runtime
behavior.
"""

from __future__ import annotations

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
CRATES = ROOT / "crates"

HARD_MARKERS = (
    re.compile(r"\btodo!\s*\("),
    re.compile(r"\bunimplemented!\s*\("),
)
PLACEHOLDER_MARKER = re.compile(r"\bplaceholder\b", re.IGNORECASE)
SKIP_PARTS = {"tests", "test", "benches", "examples"}
SCHEMA_ONLY_PLACEHOLDER_PATHS = {
    Path("crates/aurora-core/src/capability.rs"),
    Path("crates/aurora-cli/src/capabilities.rs"),
}


def iter_runtime_rust_files():
    for path in CRATES.rglob("*.rs"):
        rel = path.relative_to(ROOT)
        if any(part in SKIP_PARTS for part in rel.parts):
            continue
        if "src" not in rel.parts:
            continue
        yield path, rel


def main() -> int:
    failures: list[str] = []
    for path, rel in iter_runtime_rust_files():
        text = path.read_text(encoding="utf-8")
        for line_no, line in enumerate(text.splitlines(), 1):
            if any(marker.search(line) for marker in HARD_MARKERS):
                failures.append(f"{rel}:{line_no}: {line.strip()}")
                continue
            if rel not in SCHEMA_ONLY_PLACEHOLDER_PATHS and PLACEHOLDER_MARKER.search(line):
                failures.append(f"{rel}:{line_no}: {line.strip()}")

    if failures:
        print("Runtime implementation-stub audit FAILED:")
        for failure in failures:
            print(f"  {failure}")
        return 1

    print(
        "Runtime implementation-stub audit PASS: no todo!/unimplemented! or executable placeholder markers"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
