#!/usr/bin/env python3
"""Validate Aurora Phase 10 RoomEQ -> CamillaDSP PCM differential evidence."""

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


def parse_unsupported_log(path: Path, required_name: str) -> dict[str, Any]:
    text = path.read_text(encoding="utf-8")
    summary = re.search(
        r"test result:\s+ok\.\s+(\d+) passed;\s+0 failed;",
        text,
        flags=re.MULTILINE,
    )
    checks = {
        "required_test_named": required_name in text,
        "test_summary_present": summary is not None,
        "at_least_one_test_passed": summary is not None and int(summary.group(1)) >= 1,
        "no_failed_marker": "FAILED" not in text,
    }
    return {
        "required_test": required_name,
        "checks": checks,
        "pass": all(checks.values()),
    }


def parse_channel_sentinel(path: Path) -> dict[str, Any]:
    text = path.read_text(encoding="utf-8")
    required_name = "role_aware_real_camilladsp_preserves_center_and_surround_mapping"
    summary = re.search(
        r"test result:\s+ok\.\s+(\d+) passed;\s+0 failed;",
        text,
        flags=re.MULTILINE,
    )
    checks = {
        "required_test_named": required_name in text,
        "test_summary_present": summary is not None,
        "at_least_one_test_passed": summary is not None and int(summary.group(1)) >= 1,
        "no_failed_marker": "FAILED" not in text,
    }
    return {
        "required_test": required_name,
        "checks": checks,
        "pass": all(checks.values()),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--backend-json", type=Path, required=True)
    parser.add_argument("--unsupported-log", type=Path, required=True)
    parser.add_argument("--sentinel-log", type=Path, required=True)
    parser.add_argument("--camilladsp-lock", type=Path, required=True)
    parser.add_argument("--roomeq-head", required=True)
    parser.add_argument("--camilladsp-head", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    try:
        contract = json.loads(args.config.read_text(encoding="utf-8"))
        backend = json.loads(args.backend_json.read_text(encoding="utf-8"))
        if contract.get("schema_version") != 1 or contract.get("roadmap_phase") != 10:
            raise ValueError("unsupported CamillaDSP differential contract")
        if not args.camilladsp_lock.is_file() or args.camilladsp_lock.stat().st_size <= 0:
            raise ValueError("generated CamillaDSP dependency lock missing or empty")

        required = set(contract["required_pcm_contracts"])
        reported_required = set(backend.get("required_tests", []))
        reported_missing = set(backend.get("missing_tests", []))
        version = str(backend.get("version", ""))
        required_fail_closed = contract["fail_closed_contracts"]
        if len(required_fail_closed) != 1:
            raise ValueError("expected exactly one fail-closed contract in schema v1")

        unsupported = parse_unsupported_log(
            args.unsupported_log, required_fail_closed[0]
        )
        sentinel = parse_channel_sentinel(args.sentinel_log)
        checks = {
            "roomeq_head": args.roomeq_head == contract["roomeq"]["pinned_commit"],
            "camilladsp_head": args.camilladsp_head
            == contract["camilladsp"]["pinned_commit"],
            "camilladsp_version": contract["camilladsp"]["expected_version_fragment"]
            in version,
            "generated_camilladsp_dependency_lock_present": args.camilladsp_lock.stat().st_size > 0,
            "aurora_7_1_4_channel_sentinel": sentinel["pass"],
            "backend_status": backend.get("status") == "passed",
            "backend_returncode": backend.get("returncode") == 0,
            "required_contract_set_exact": reported_required == required,
            "missing_contracts_empty": not reported_missing,
            "required_contract_count": int(backend.get("tests_passed", 0)) >= len(required),
            "unsupported_feature_rejected": unsupported["pass"],
        }
        verdict = all(checks.values())
        evidence = {
            "schema_version": 1,
            "roadmap_phase": 10,
            "pins": {
                "roomeq": args.roomeq_head,
                "camilladsp": args.camilladsp_head,
            },
            "camilladsp_version": version,
            "camilladsp_dependency_lock": {
                "policy": contract["camilladsp"]["dependency_lock_policy"],
                "sha256": sha256(args.camilladsp_lock),
                "bytes": args.camilladsp_lock.stat().st_size,
            },
            "aurora_channel_sentinel": sentinel,
            "backend": {
                "status": backend.get("status"),
                "returncode": backend.get("returncode"),
                "tests_passed": backend.get("tests_passed"),
                "required_tests": sorted(reported_required),
                "missing_tests": sorted(reported_missing),
                "scope": backend.get("scope"),
            },
            "unsupported_feature_contract": unsupported,
            "checks": checks,
            "hashes": {
                "contract": sha256(args.config),
                "backend_json": sha256(args.backend_json),
                "unsupported_log": sha256(args.unsupported_log),
                "sentinel_log": sha256(args.sentinel_log),
                "camilladsp_generated_lock": sha256(args.camilladsp_lock),
            },
            "truth_boundary": contract["truth_boundary"],
            "verdict": "pass" if verdict else "fail",
        }
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
        verdict = False
        evidence = {
            "schema_version": 1,
            "roadmap_phase": 10,
            "verdict": "fail",
            "error": str(exc),
        }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    print(json.dumps(evidence, indent=2, sort_keys=True))
    return 0 if verdict else 1


if __name__ == "__main__":
    sys.exit(main())
