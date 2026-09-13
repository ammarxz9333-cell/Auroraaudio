#!/usr/bin/env python3
"""Validate Aurora's mandatory full-system simulation coverage contract."""

from __future__ import annotations

import argparse
import copy
import importlib.util
import json
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CONTRACT = ROOT / "config/simulation-coverage-v1.json"
SIMULATOR = Path(__file__).resolve().with_name("aurora_full_system_sim.py")
VALID_STATUSES = {"covered", "planned", "physical-pending", "external-pending"}
VALID_CLASSES = {"virtual", "software_reference", "physical", "external_integration"}


def load_simulator():
    spec = importlib.util.spec_from_file_location("aurora_full_system_sim", SIMULATOR)
    if spec is None or spec.loader is None:
        raise RuntimeError("unable to load Aurora full-system simulator module")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_contract(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError("coverage contract must be a JSON object")
    return payload


def _nonempty_strings(value: Any) -> bool:
    return isinstance(value, list) and bool(value) and all(isinstance(item, str) and item for item in value)


def validate(contract: dict[str, Any], simulator: Any, *, check_paths: bool = True) -> list[str]:
    errors: list[str] = []
    if contract.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    capabilities = contract.get("capabilities")
    if not isinstance(capabilities, list) or not capabilities:
        return errors + ["capabilities must be a non-empty list"]

    simulator_caps = set(getattr(simulator, "SIMULATOR_CAPABILITIES", ()))
    fault_profiles = set(getattr(simulator, "FAULT_PROFILES", ()))
    if not simulator_caps:
        errors.append("simulator exports no SIMULATOR_CAPABILITIES")
    if "none" not in fault_profiles:
        errors.append("simulator fault profiles must contain healthy profile 'none'")

    seen_ids: set[str] = set()
    mapped_sim_caps: dict[str, str] = {}
    mapped_faults: set[str] = set()

    for index, item in enumerate(capabilities):
        prefix = f"capabilities[{index}]"
        if not isinstance(item, dict):
            errors.append(f"{prefix} must be an object")
            continue
        cid = item.get("id")
        status = item.get("status")
        evidence_class = item.get("evidence_class")
        if not isinstance(cid, str) or not cid:
            errors.append(f"{prefix}.id must be a non-empty string")
            continue
        if cid in seen_ids:
            errors.append(f"duplicate capability id: {cid}")
        seen_ids.add(cid)
        if status not in VALID_STATUSES:
            errors.append(f"{cid}: invalid status {status!r}")
        if evidence_class not in VALID_CLASSES:
            errors.append(f"{cid}: invalid evidence_class {evidence_class!r}")

        if status == "covered":
            evidence = item.get("evidence")
            if not _nonempty_strings(evidence):
                errors.append(f"{cid}: covered capability requires non-empty evidence paths")
            elif check_paths:
                for rel in evidence:
                    if not (ROOT / rel).exists():
                        errors.append(f"{cid}: evidence path does not exist: {rel}")

            if evidence_class == "virtual":
                sim_cap = item.get("simulator_capability")
                profiles = item.get("profiles")
                if not isinstance(sim_cap, str) or not sim_cap:
                    errors.append(f"{cid}: covered virtual capability requires simulator_capability")
                elif sim_cap not in simulator_caps:
                    errors.append(f"{cid}: unknown simulator capability {sim_cap!r}")
                elif sim_cap in mapped_sim_caps:
                    errors.append(
                        f"simulator capability {sim_cap!r} mapped by both {mapped_sim_caps[sim_cap]!r} and {cid!r}"
                    )
                else:
                    mapped_sim_caps[sim_cap] = cid
                if not _nonempty_strings(profiles):
                    errors.append(f"{cid}: covered virtual capability requires profiles")
                else:
                    for profile in profiles:
                        if profile not in fault_profiles:
                            errors.append(f"{cid}: unknown fault profile {profile!r}")
                        else:
                            mapped_faults.add(profile)
            elif evidence_class == "software_reference":
                pass
            else:
                errors.append(f"{cid}: physical/external capability cannot be status=covered in software contract")
        else:
            truth = item.get("truth_boundary")
            if not isinstance(truth, str) or not truth.strip():
                errors.append(f"{cid}: non-covered capability requires explicit truth_boundary")
            if status == "physical-pending" and evidence_class != "physical":
                errors.append(f"{cid}: physical-pending must use evidence_class=physical")
            if status == "external-pending" and evidence_class != "external_integration":
                errors.append(f"{cid}: external-pending must use evidence_class=external_integration")

    missing_caps = simulator_caps - set(mapped_sim_caps)
    extra_caps = set(mapped_sim_caps) - simulator_caps
    if missing_caps:
        errors.append("simulator capabilities missing from contract: " + ",".join(sorted(missing_caps)))
    if extra_caps:
        errors.append("contract references unknown simulator capabilities: " + ",".join(sorted(extra_caps)))

    required_faults = fault_profiles - {"none"}
    missing_faults = required_faults - mapped_faults
    if missing_faults:
        errors.append("fault profiles missing from covered capability mapping: " + ",".join(sorted(missing_faults)))
    return errors


def self_test() -> None:
    simulator = load_simulator()
    contract = load_contract(DEFAULT_CONTRACT)
    errors = validate(contract, simulator)
    if errors:
        raise AssertionError("real coverage contract failed validation: " + "; ".join(errors))

    duplicate = copy.deepcopy(contract)
    duplicate["capabilities"].append(copy.deepcopy(duplicate["capabilities"][0]))
    if not any("duplicate capability id" in error for error in validate(duplicate, simulator, check_paths=False)):
        raise AssertionError("duplicate capability was not rejected")

    missing = copy.deepcopy(contract)
    sim_cap = next(item["simulator_capability"] for item in missing["capabilities"]
                   if item.get("status") == "covered" and item.get("evidence_class") == "virtual")
    missing["capabilities"] = [
        item for item in missing["capabilities"] if item.get("simulator_capability") != sim_cap
    ]
    if not any("simulator capabilities missing" in error for error in validate(missing, simulator, check_paths=False)):
        raise AssertionError("missing simulator capability was not rejected")

    bad_fault = copy.deepcopy(contract)
    for item in bad_fault["capabilities"]:
        if item.get("status") == "covered" and item.get("evidence_class") == "virtual":
            item["profiles"] = ["not-a-real-profile"]
            break
    if not any("unknown fault profile" in error for error in validate(bad_fault, simulator, check_paths=False)):
        raise AssertionError("unknown fault profile was not rejected")

    print("AURORA-SIMULATION-COVERAGE-SELF-TEST-PASS")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("self-test")
    check = sub.add_parser("check")
    check.add_argument("--contract", type=Path, default=DEFAULT_CONTRACT)
    args = parser.parse_args()

    if args.command == "self-test":
        self_test()
        return 0

    simulator = load_simulator()
    contract = load_contract(args.contract)
    errors = validate(contract, simulator)
    if errors:
        for error in errors:
            print(f"AURORA-SIMULATION-COVERAGE-ERROR {error}", file=sys.stderr)
        return 1
    print(
        "AURORA-SIMULATION-COVERAGE-PASS "
        f"capabilities={len(contract['capabilities'])} simulator_capabilities={len(simulator.SIMULATOR_CAPABILITIES)} "
        f"fault_profiles={len(simulator.FAULT_PROFILES)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
