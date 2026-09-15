#!/usr/bin/env python3
"""Validate Aurora's pinned Phase 10 room-correction references.

This script deliberately validates only source identity, license/version metadata,
required source surfaces and evidence provenance. Upstream executable tests are
run separately by CI before this analyzer is invoked.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any


def git_head(path: Path) -> str:
    return subprocess.check_output(
        ["git", "-C", str(path), "rev-parse", "HEAD"], text=True
    ).strip()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def require_surfaces(root: Path, surfaces: list[str]) -> tuple[bool, list[str]]:
    missing = [surface for surface in surfaces if not (root / surface).exists()]
    return not missing, missing


def validate_roomeq(root: Path, contract: dict[str, Any]) -> dict[str, Any]:
    cargo = load_toml(root / "Cargo.toml")
    actual_commit = git_head(root)
    actual_license = cargo["package"].get("license")
    actual_version = cargo.get("workspace", {}).get("package", {}).get("version")
    surfaces_ok, missing = require_surfaces(root, contract["required_surfaces"])
    checks = {
        "commit": actual_commit == contract["pinned_commit"],
        "license": actual_license == contract["license"],
        "version": actual_version == contract["observed_version"],
        "required_surfaces": surfaces_ok,
    }
    return {
        "expected_commit": contract["pinned_commit"],
        "actual_commit": actual_commit,
        "expected_license": contract["license"],
        "actual_license": actual_license,
        "expected_version": contract["observed_version"],
        "actual_version": actual_version,
        "missing_surfaces": missing,
        "checks": checks,
        "pass": all(checks.values()),
    }


def validate_camilladsp(root: Path, contract: dict[str, Any]) -> dict[str, Any]:
    cargo = load_toml(root / "Cargo.toml")
    actual_commit = git_head(root)
    actual_license = cargo["package"].get("license")
    actual_version = cargo["package"].get("version")
    surfaces_ok, missing = require_surfaces(root, contract["required_surfaces"])
    checks = {
        "commit": actual_commit == contract["pinned_commit"],
        "license": actual_license == contract["license"],
        "version": actual_version == contract["observed_version"],
        "required_surfaces": surfaces_ok,
    }
    return {
        "expected_commit": contract["pinned_commit"],
        "actual_commit": actual_commit,
        "expected_license": contract["license"],
        "actual_license": actual_license,
        "expected_version": contract["observed_version"],
        "actual_version": actual_version,
        "missing_surfaces": missing,
        "checks": checks,
        "pass": all(checks.values()),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--autoeq", type=Path, required=True)
    parser.add_argument("--camilladsp", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    contract = json.loads(args.config.read_text(encoding="utf-8"))
    if contract.get("schema_version") != 1 or contract.get("roadmap_phase") != 10:
        raise SystemExit("unsupported room-correction reference contract")

    roomeq = validate_roomeq(args.autoeq, contract["references"]["roomeq"])
    camilladsp = validate_camilladsp(
        args.camilladsp, contract["references"]["camilladsp"]
    )
    verdict = roomeq["pass"] and camilladsp["pass"]
    evidence = {
        "schema_version": 1,
        "roadmap_phase": 10,
        "contract_sha256": sha256(args.config),
        "roomeq": roomeq,
        "camilladsp": camilladsp,
        "upstream_tests": {
            "required_before_evidence": True,
            "roomeq": contract["references"]["roomeq"]["selected_upstream_tests"],
            "camilladsp": contract["references"]["camilladsp"]["selected_upstream_tests"],
        },
        "truth_boundary": contract["truth_boundary"],
        "verdict": "pass" if verdict else "fail",
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    print(json.dumps(evidence, indent=2, sort_keys=True))
    return 0 if verdict else 1


if __name__ == "__main__":
    sys.exit(main())
