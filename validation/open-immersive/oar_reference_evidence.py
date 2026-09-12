#!/usr/bin/env python3
"""Validate and summarize the pinned AOMedia OAR reference build/test lane."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path


def git_head(repo: Path) -> str:
    return subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()


def parse_ctest(log: Path) -> tuple[int, int]:
    text = log.read_text(encoding="utf-8", errors="replace")
    match = re.search(r"(\d+)% tests passed,\s+(\d+) tests failed out of\s+(\d+)", text)
    if not match:
        raise ValueError("CTest summary not found")
    percent, failed, total = map(int, match.groups())
    if total <= 0:
        raise ValueError("CTest executed zero tests")
    passed = total - failed
    if percent != 100 or failed != 0 or passed != total:
        raise ValueError(f"upstream OAR tests failed: passed={passed} failed={failed} total={total}")
    return passed, total


def analyze(config_path: Path, repo: Path, ctest_log: Path, output: Path) -> int:
    config = json.loads(config_path.read_text(encoding="utf-8"))
    ref = config["reference"]
    expected_commit = ref["commit"]
    actual_commit = git_head(repo)
    failures: list[str] = []

    if actual_commit != expected_commit:
        failures.append(f"commit mismatch: expected {expected_commit} got {actual_commit}")

    license_path = repo / "LICENSE"
    patents_path = repo / "PATENTS"
    if not license_path.is_file():
        failures.append("LICENSE missing")
    if ref.get("patent_file_required") and not patents_path.is_file():
        failures.append("PATENTS missing")

    passed = total = 0
    try:
        passed, total = parse_ctest(ctest_log)
    except Exception as exc:
        failures.append(str(exc))

    report = {
        "schema_version": 1,
        "verdict": "pass" if not failures else "reject",
        "reference": {
            "id": ref["id"],
            "version": ref["version"],
            "upstream": ref["upstream"],
            "expected_commit": expected_commit,
            "actual_commit": actual_commit,
            "license": ref["license"],
            "license_present": license_path.is_file(),
            "patents_present": patents_path.is_file(),
            "integration": ref["integration"],
        },
        "upstream_tests": {"passed": passed, "total": total},
        "failures": failures,
        "truth_boundary": config["truth_boundary"],
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        f"AURORA-OAR-REFERENCE-{report['verdict'].upper()} "
        f"commit={actual_commit} tests={passed}/{total}"
    )
    for failure in failures:
        print(f"REJECT: {failure}")
    return 0 if not failures else 1


def self_test() -> int:
    sample = """Test project /tmp/build\n    Start 1: a\n1/2 Test #1: a ... Passed\n    Start 2: b\n2/2 Test #2: b ... Passed\n\n100% tests passed, 0 tests failed out of 2\n"""
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "ctest.log"
        path.write_text(sample, encoding="utf-8")
        passed, total = parse_ctest(path)
        if (passed, total) != (2, 2):
            raise AssertionError((passed, total))
    print("AURORA-OAR-REFERENCE-EVIDENCE-SELFTEST-PASS")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("self-test")
    analyze_parser = sub.add_parser("analyze")
    analyze_parser.add_argument("--config", type=Path, required=True)
    analyze_parser.add_argument("--repo", type=Path, required=True)
    analyze_parser.add_argument("--ctest-log", type=Path, required=True)
    analyze_parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "self-test":
        return self_test()
    return analyze(args.config, args.repo, args.ctest_log, args.output)


if __name__ == "__main__":
    raise SystemExit(main())
