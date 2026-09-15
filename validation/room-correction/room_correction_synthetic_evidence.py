#!/usr/bin/env python3
"""Fail-closed analyzer for Aurora Phase 10 synthetic RoomEQ evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def one(pattern: str, text: str, label: str) -> re.Match[str]:
    match = re.search(pattern, text, flags=re.MULTILINE)
    if match is None:
        raise ValueError(f"missing required {label} marker")
    return match


def parse_chain(text: str) -> dict[str, Any]:
    summary = one(
        r"test result:\s+ok\.\s+(\d+) passed;\s+0 failed;",
        text,
        "chain-constraint test summary",
    )
    passed = int(summary.group(1))
    checks = {
        "nonempty": passed > 0,
        "constraint_module_exercised": "chain_constraints_tests::" in text,
        "no_failed_test": "FAILED" not in text,
    }
    return {"passed": passed, "checks": checks, "pass": all(checks.values())}


def parse_multichannel(text: str, expected_cases: int) -> dict[str, Any]:
    results = one(
        r"Results:\s+(\d+) passed,\s+(\d+) failed,\s+(\d+) total",
        text,
        "multichannel results",
    )
    passed, failed, total = map(int, results.groups())
    mc = one(
        r"multi-channel:\s+(\d+)/(\d+) passed",
        text,
        "multi-channel summary",
    )
    mc_passed, mc_total = map(int, mc.groups())
    outcomes = one(
        r"Outcome summary:\s+PASS=(\d+),\s+REVERTED=(\d+),\s+FAIL=(\d+)",
        text,
        "outcome summary",
    )
    outcome_pass, reverted, outcome_fail = map(int, outcomes.groups())
    checks = {
        "overall_failed_zero": failed == 0,
        "overall_counts_consistent": passed + failed == total,
        "multichannel_case_count": mc_total == expected_cases,
        "multichannel_all_pass": mc_passed == mc_total == expected_cases,
        "outcome_fail_zero": outcome_fail == 0,
        "outcome_counts_consistent": outcome_pass + reverted + outcome_fail == total,
        "no_fail_marker": "FAIL:" not in text,
    }
    return {
        "passed": passed,
        "failed": failed,
        "total": total,
        "multichannel_passed": mc_passed,
        "multichannel_total": mc_total,
        "outcome_pass": outcome_pass,
        "safe_reverted": reverted,
        "outcome_fail": outcome_fail,
        "checks": checks,
        "pass": all(checks.values()),
    }


def parse_multiseat(text: str) -> dict[str, Any]:
    results = one(
        r"Results:\s+(\d+) passed,\s+(\d+) failed,\s+(\d+) total",
        text,
        "multi-seat results",
    )
    passed, failed, total = map(int, results.groups())
    checks = {
        "nonempty": total > 0,
        "failed_zero": failed == 0,
        "all_pass": passed == total,
        "counts_consistent": passed + failed == total,
        "missing_phase_guard_present": "missing_phase_rejected" in text,
        "phase_control_guard_present": "polarity/all-pass" in text,
    }
    return {
        "passed": passed,
        "failed": failed,
        "total": total,
        "checks": checks,
        "pass": all(checks.values()),
    }


def parse_stage3(text: str) -> dict[str, Any]:
    summary = one(
        r"stage3 policies:\s+(\d+)/(\d+) demo expectations hold",
        text,
        "Stage 3 policy summary",
    )
    passed, total = map(int, summary.groups())
    checks = {
        "nonempty": total > 0,
        "all_expectations_hold": passed == total,
        "no_demo_failure": "stage3 demo FAIL:" not in text,
    }
    return {
        "passed": passed,
        "total": total,
        "checks": checks,
        "pass": all(checks.values()),
    }


def normalized_commit(value: object, label: str) -> str:
    commit = str(value).strip().lower()
    if re.fullmatch(r"[0-9a-f]{40}", commit) is None:
        raise ValueError(f"{label} must be a full 40-character lowercase-compatible git SHA")
    return commit


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--chain-log", type=Path, required=True)
    parser.add_argument("--multichannel-log", type=Path, required=True)
    parser.add_argument("--multiseat-log", type=Path, required=True)
    parser.add_argument("--stage3-log", type=Path, required=True)
    parser.add_argument("--roomeq-head", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    try:
        contract = json.loads(args.config.read_text(encoding="utf-8"))
        if contract.get("schema_version") != 1 or contract.get("roadmap_phase") != 10:
            raise ValueError("unsupported synthetic room-correction contract")
        contract_pin = normalized_commit(contract["roomeq"]["pinned_commit"], "contract RoomEQ pin")
        tested_head = normalized_commit(args.roomeq_head, "tested RoomEQ checkout")
        if contract_pin != tested_head:
            raise ValueError(
                f"RoomEQ contract pin {contract_pin} does not match tested checkout {tested_head}"
            )
        expected_cases = int(
            contract["cases"]["multichannel_7_1_4"]["expected_multichannel_cases"]
        )
        chain_text = args.chain_log.read_text(encoding="utf-8")
        multichannel_text = args.multichannel_log.read_text(encoding="utf-8")
        multiseat_text = args.multiseat_log.read_text(encoding="utf-8")
        stage3_text = args.stage3_log.read_text(encoding="utf-8")
        chain = parse_chain(chain_text)
        multichannel = parse_multichannel(multichannel_text, expected_cases)
        multiseat = parse_multiseat(multiseat_text)
        stage3 = parse_stage3(stage3_text)
        verdict = (
            chain["pass"]
            and multichannel["pass"]
            and multiseat["pass"]
            and stage3["pass"]
        )
        evidence = {
            "schema_version": 1,
            "roadmap_phase": 10,
            "roomeq_pinned_commit": contract_pin,
            "roomeq_tested_commit": tested_head,
            "contract_sha256": sha256(args.config),
            "logs_sha256": {
                "chain": sha256(args.chain_log),
                "multichannel": sha256(args.multichannel_log),
                "multiseat": sha256(args.multiseat_log),
                "stage3": sha256(args.stage3_log),
            },
            "chain_constraints": chain,
            "multichannel": multichannel,
            "multiseat": multiseat,
            "stage3": stage3,
            "truth_boundary": contract["truth_boundary"],
            "verdict": "pass" if verdict else "fail",
        }
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
        evidence = {
            "schema_version": 1,
            "roadmap_phase": 10,
            "verdict": "fail",
            "error": str(exc),
        }
        verdict = False

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    print(json.dumps(evidence, indent=2, sort_keys=True))
    return 0 if verdict else 1


if __name__ == "__main__":
    sys.exit(main())
