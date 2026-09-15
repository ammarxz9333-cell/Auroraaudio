#!/usr/bin/env python3
"""Validate Aurora's pinned Phase 11 binaural reference baseline."""

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


def git_object(path: Path, spec: str) -> str:
    return subprocess.check_output(
        ["git", "-C", str(path), "rev-parse", spec], text=True
    ).strip()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require_surfaces(root: Path, surfaces: list[str]) -> tuple[bool, list[str]]:
    missing = [surface for surface in surfaces if not (root / surface).exists()]
    return not missing, missing


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def validate_registry(
    registry: dict[str, Any], references: dict[str, Any]
) -> dict[str, Any]:
    reference_to_component = {
        "google_obr": "google-obr",
        "ebu_bear": "ebu-bear",
        "sofar": "sofar",
    }
    components = registry.get("components", [])
    ids = [component.get("id") for component in components]
    duplicate_ids = sorted({component_id for component_id in ids if ids.count(component_id) > 1})
    entries = {component.get("id"): component for component in components}
    pin_checks: dict[str, bool] = {}
    actual_pins: dict[str, str | None] = {}
    expected_pins: dict[str, str] = {}
    missing_components: list[str] = []

    for reference_name, component_id in reference_to_component.items():
        expected = references[reference_name]["pinned_commit"]
        entry = entries.get(component_id)
        actual = entry.get("pinned_commit") if entry else None
        expected_pins[component_id] = expected
        actual_pins[component_id] = actual
        pin_checks[component_id] = actual == expected
        if entry is None:
            missing_components.append(component_id)

    checks = {
        "schema_version": registry.get("schema_version") == 1,
        "unique_component_ids": not duplicate_ids,
        "required_components_present": not missing_components,
        "pinned_commits_match_contract": all(pin_checks.values()),
    }
    return {
        "expected_pins": expected_pins,
        "actual_pins": actual_pins,
        "pin_checks": pin_checks,
        "missing_components": missing_components,
        "duplicate_component_ids": duplicate_ids,
        "checks": checks,
        "pass": all(checks.values()),
    }


def validate_obr(root: Path, contract: dict[str, Any]) -> dict[str, Any]:
    actual_commit = git_head(root)
    surfaces_ok, missing = require_surfaces(root, contract["required_surfaces"])
    license_text = (root / "LICENSE").read_text(encoding="utf-8", errors="replace")
    patents_text = (root / "PATENTS").read_text(encoding="utf-8", errors="replace")
    checks = {
        "commit": actual_commit == contract["pinned_commit"],
        "required_surfaces": surfaces_ok,
        "bsd_license_surface": "Redistribution and use in source and binary forms" in license_text,
        "patent_license_surface": "Open Binaural Renderer Patent License 1.0" in patents_text,
    }
    return {
        "expected_commit": contract["pinned_commit"],
        "actual_commit": actual_commit,
        "declared_license_boundary": contract["license"],
        "missing_surfaces": missing,
        "checks": checks,
        "pass": all(checks.values()),
    }


def validate_bear(root: Path, contract: dict[str, Any]) -> dict[str, Any]:
    actual_commit = git_head(root)
    surfaces_ok, missing = require_surfaces(root, contract["required_surfaces"])
    license_text = (root / "LICENSE").read_text(encoding="utf-8", errors="replace")
    readme_text = (root / "README.md").read_text(encoding="utf-8", errors="replace")
    flake_lock = json.loads((root / "flake.lock").read_text(encoding="utf-8"))
    actual_revisions: dict[str, str | None] = {}
    revision_checks: dict[str, bool] = {}
    for name, expected in contract["locked_flake_revisions"].items():
        actual = flake_lock.get("nodes", {}).get(name, {}).get("locked", {}).get("rev")
        actual_revisions[name] = actual
        revision_checks[name] = actual == expected
    checks = {
        "commit": actual_commit == contract["pinned_commit"],
        "required_surfaces": surfaces_ok,
        "license": "Apache License" in license_text and "Version 2.0" in license_text,
        "maturity_boundary": "pre-release" in readme_text.lower(),
        "locked_flake_revisions": all(revision_checks.values()),
    }
    return {
        "expected_commit": contract["pinned_commit"],
        "actual_commit": actual_commit,
        "declared_license_boundary": contract["license"],
        "declared_maturity": contract["maturity"],
        "expected_flake_revisions": contract["locked_flake_revisions"],
        "actual_flake_revisions": actual_revisions,
        "flake_revision_checks": revision_checks,
        "flake_lock_sha256": sha256(root / "flake.lock"),
        "missing_surfaces": missing,
        "checks": checks,
        "pass": all(checks.values()),
    }


def validate_sofar(root: Path, contract: dict[str, Any]) -> dict[str, Any]:
    cargo = load_toml(root / "Cargo.toml")
    lock = root / "Cargo.lock"
    actual_commit = git_head(root)
    actual_version = cargo["package"].get("version")
    actual_license = cargo["package"].get("license")
    surfaces_ok, missing = require_surfaces(root, contract["required_surfaces"])
    actual_submodule = git_object(root, "HEAD:libmysofa-sys/libmysofa")
    checks = {
        "commit": actual_commit == contract["pinned_commit"],
        "version": actual_version == contract["observed_version"],
        "license": actual_license == contract["license"],
        "required_surfaces": surfaces_ok,
        "libmysofa_submodule": actual_submodule == contract["libmysofa_submodule_commit"],
        "generated_dependency_lock_present": lock.is_file(),
    }
    return {
        "expected_commit": contract["pinned_commit"],
        "actual_commit": actual_commit,
        "expected_version": contract["observed_version"],
        "actual_version": actual_version,
        "expected_license": contract["license"],
        "actual_license": actual_license,
        "expected_libmysofa_submodule_commit": contract["libmysofa_submodule_commit"],
        "actual_libmysofa_submodule_commit": actual_submodule,
        "dependency_lock_policy": contract["dependency_lock_policy"],
        "generated_dependency_lock_sha256": sha256(lock) if lock.is_file() else None,
        "missing_surfaces": missing,
        "checks": checks,
        "pass": all(checks.values()),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--obr", type=Path, required=True)
    parser.add_argument("--bear", type=Path, required=True)
    parser.add_argument("--sofar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    contract = json.loads(args.config.read_text(encoding="utf-8"))
    registry_data = json.loads(args.registry.read_text(encoding="utf-8"))
    if contract.get("schema_version") != 1 or contract.get("roadmap_phase") != 11:
        raise SystemExit("unsupported binaural reference contract")

    references = contract["references"]
    registry = validate_registry(registry_data, references)
    obr = validate_obr(args.obr, references["google_obr"])
    bear = validate_bear(args.bear, references["ebu_bear"])
    sofar = validate_sofar(args.sofar, references["sofar"])
    verdict = registry["pass"] and obr["pass"] and bear["pass"] and sofar["pass"]
    evidence = {
        "schema_version": 1,
        "roadmap_phase": 11,
        "contract_sha256": sha256(args.config),
        "external_registry_sha256": sha256(args.registry),
        "registry_consistency": registry,
        "obr": obr,
        "bear": bear,
        "sofar": sofar,
        "upstream_execution_gates": {
            "required_before_evidence": True,
            "obr": references["google_obr"]["selected_upstream_tests"],
            "bear": references["ebu_bear"]["selected_upstream_tests"],
            "sofar": references["sofar"]["selected_upstream_tests"],
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
