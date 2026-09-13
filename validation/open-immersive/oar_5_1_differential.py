#!/usr/bin/env python3
"""Invariant-based Aurora-vs-OAR 5.1 object-rendering evidence.

The comparison uses semantic speaker targets and normalized channel energy rather
than raw PCM identity. LFE is treated specially: point-object rendering must
preserve the LFE output slot but must not pan object energy into it.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any

REQUIRED_CASES = (
    "front_left_unity",
    "center_unity",
    "front_right_unity",
    "surround_left_unity",
    "surround_right_unity",
    "center_minus_6db",
)
EXPECTED_SEMANTICS = {
    "front_left_unity": "front_left",
    "center_unity": "center",
    "front_right_unity": "front_right",
    "surround_left_unity": "surround_left",
    "surround_right_unity": "surround_right",
    "center_minus_6db": "center",
}
TARGET_CHANNEL = {
    "front_left_unity": 0,
    "center_unity": 2,
    "front_right_unity": 1,
    "surround_left_unity": 4,
    "surround_right_unity": 5,
    "center_minus_6db": 2,
}
NON_LFE_CHANNELS = (0, 1, 2, 4, 5)
LFE_INDEX = 3


class DifferentialError(RuntimeError):
    pass


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise DifferentialError(f"failed to read {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise DifferentialError(f"{path} must contain a JSON object")
    return value


def finite_number(value: Any, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise DifferentialError(f"{label} must be numeric")
    number = float(value)
    if not math.isfinite(number):
        raise DifferentialError(f"{label} must be finite")
    return number


def case_map(document: dict[str, Any], label: str) -> dict[str, dict[str, Any]]:
    cases = document.get("cases")
    if not isinstance(cases, list):
        raise DifferentialError(f"{label}: cases must be an array")
    mapped: dict[str, dict[str, Any]] = {}
    for case in cases:
        if not isinstance(case, dict) or not isinstance(case.get("name"), str):
            raise DifferentialError(f"{label}: every case needs a string name")
        name = case["name"]
        if name in mapped:
            raise DifferentialError(f"{label}: duplicate case {name}")
        mapped[name] = case
    if tuple(mapped) != REQUIRED_CASES:
        raise DifferentialError(
            f"{label}: case order/names must be {list(REQUIRED_CASES)}, got {list(mapped)}"
        )
    return mapped


def validate_probe(
    document: dict[str, Any], config: dict[str, Any], label: str
) -> dict[str, dict[str, Any]]:
    contract = config["multichannel_differential"]
    if document.get("schema_version") != 1:
        raise DifferentialError(f"{label}: schema_version must be 1")
    if document.get("layout") != contract["layout"]:
        raise DifferentialError(f"{label}: unexpected layout {document.get('layout')!r}")
    if document.get("sample_rate") != contract["sample_rate"]:
        raise DifferentialError(f"{label}: unexpected sample rate")
    if document.get("frames_per_case") != contract["frames_per_case"]:
        raise DifferentialError(f"{label}: unexpected frame count")
    if document.get("channel_order") != contract["channel_order"]:
        raise DifferentialError(
            f"{label}: channel order must be {contract['channel_order']}, got {document.get('channel_order')!r}"
        )
    if document.get("lfe_semantics") != "non-directional-zero-for-object-render":
        raise DifferentialError(f"{label}: missing object-render LFE semantic declaration")

    cases = case_map(document, label)
    for name, case in cases.items():
        if case.get("semantic_position") != EXPECTED_SEMANTICS[name]:
            raise DifferentialError(f"{label}/{name}: unexpected semantic_position")
        if case.get("frame_count") != contract["frames_per_case"]:
            raise DifferentialError(f"{label}/{name}: frame accounting mismatch")
        if case.get("finite") is not True:
            raise DifferentialError(f"{label}/{name}: renderer reported non-finite output")
        levels = case.get("channel_levels")
        if not isinstance(levels, list) or len(levels) != 6:
            raise DifferentialError(f"{label}/{name}: channel_levels must contain 6 channels")
        for index, value in enumerate(levels):
            level = finite_number(value, f"{label}/{name}/channel_levels[{index}]")
            if level < 0.0:
                raise DifferentialError(f"{label}/{name}: channel level cannot be negative")
        finite_number(case.get("azimuth_degrees"), f"{label}/{name}/azimuth_degrees")
        finite_number(case.get("gain_db"), f"{label}/{name}/gain_db")
    return cases


def non_lfe_norm(levels: list[Any], label: str) -> tuple[float, ...]:
    values = [finite_number(levels[index], f"{label}/{index}") for index in NON_LFE_CHANNELS]
    norm = math.sqrt(sum(value * value for value in values))
    if norm <= 1.0e-12:
        raise DifferentialError(f"{label}: non-LFE output is effectively silent")
    return tuple(value / norm for value in values)


def non_lfe_total(case: dict[str, Any], label: str) -> float:
    levels = case["channel_levels"]
    total = math.sqrt(
        sum(
            finite_number(levels[index], f"{label}/{index}") ** 2
            for index in NON_LFE_CHANNELS
        )
    )
    if total <= 1.0e-12:
        raise DifferentialError(f"{label}: non-LFE total is effectively silent")
    return total


def gain_delta_db(cases: dict[str, dict[str, Any]], label: str) -> float:
    unity = non_lfe_total(cases["center_unity"], f"{label}/center_unity")
    attenuated = non_lfe_total(
        cases["center_minus_6db"], f"{label}/center_minus_6db"
    )
    return 20.0 * math.log10(attenuated / unity)


def analyze_documents(
    config: dict[str, Any], aurora: dict[str, Any], oar: dict[str, Any]
) -> dict[str, Any]:
    contract = config.get("multichannel_differential")
    if not isinstance(contract, dict):
        raise DifferentialError("config is missing multichannel_differential contract")

    aurora_cases = validate_probe(aurora, config, "aurora")
    oar_cases = validate_probe(oar, config, "oar")
    checks: dict[str, bool] = {}
    failures: list[str] = []
    metrics: dict[str, Any] = {
        "target_channel_fraction": {},
        "normalized_channel_delta": {},
        "lfe_relative_level": {},
    }

    target_min = finite_number(contract["target_channel_min_fraction"], "target_channel_min_fraction")
    spatial_tolerance = finite_number(
        contract["normalized_channel_tolerance"], "normalized_channel_tolerance"
    )
    lfe_max = finite_number(contract["lfe_relative_max"], "lfe_relative_max")

    for implementation, cases in (("aurora", aurora_cases), ("oar", oar_cases)):
        metrics["target_channel_fraction"][implementation] = {}
        metrics["lfe_relative_level"][implementation] = {}
        for name in REQUIRED_CASES:
            levels = cases[name]["channel_levels"]
            normalized = non_lfe_norm(levels, f"{implementation}/{name}")
            target_full_index = TARGET_CHANNEL[name]
            target_non_lfe_index = NON_LFE_CHANNELS.index(target_full_index)
            target_fraction = normalized[target_non_lfe_index]
            lfe_level = finite_number(levels[LFE_INDEX], f"{implementation}/{name}/LFE")
            non_lfe_level = non_lfe_total(cases[name], f"{implementation}/{name}")
            lfe_relative = lfe_level / non_lfe_level
            metrics["target_channel_fraction"][implementation][name] = target_fraction
            metrics["lfe_relative_level"][implementation][name] = lfe_relative
            checks[f"{implementation}_target_{name}"] = target_fraction >= target_min
            checks[f"{implementation}_lfe_silent_{name}"] = lfe_relative <= lfe_max

    for name in REQUIRED_CASES:
        aurora_normalized = non_lfe_norm(
            aurora_cases[name]["channel_levels"], f"aurora/{name}"
        )
        oar_normalized = non_lfe_norm(oar_cases[name]["channel_levels"], f"oar/{name}")
        delta = max(abs(a - b) for a, b in zip(aurora_normalized, oar_normalized))
        metrics["normalized_channel_delta"][name] = delta
        checks[f"normalized_spatial_match_{name}"] = delta <= spatial_tolerance

    target_gain_db = finite_number(contract["expected_gain_db"], "expected_gain_db")
    gain_tolerance = finite_number(contract["gain_db_tolerance"], "gain_db_tolerance")
    cross_gain_tolerance = finite_number(
        contract["cross_renderer_gain_db_tolerance"],
        "cross_renderer_gain_db_tolerance",
    )
    aurora_gain_db = gain_delta_db(aurora_cases, "aurora")
    oar_gain_db = gain_delta_db(oar_cases, "oar")
    metrics["gain_delta_db"] = {
        "target": target_gain_db,
        "aurora": aurora_gain_db,
        "oar": oar_gain_db,
        "cross_renderer_delta": abs(aurora_gain_db - oar_gain_db),
    }
    checks["aurora_gain_semantics"] = abs(aurora_gain_db - target_gain_db) <= gain_tolerance
    checks["oar_gain_semantics"] = abs(oar_gain_db - target_gain_db) <= gain_tolerance
    checks["cross_renderer_gain_match"] = abs(aurora_gain_db - oar_gain_db) <= cross_gain_tolerance
    checks["finite_output"] = all(
        case["finite"] is True
        for cases in (aurora_cases, oar_cases)
        for case in cases.values()
    )
    checks["frame_accounting"] = all(
        case["frame_count"] == contract["frames_per_case"]
        for cases in (aurora_cases, oar_cases)
        for case in cases.values()
    )
    checks["channel_order"] = (
        aurora["channel_order"] == contract["channel_order"]
        and oar["channel_order"] == contract["channel_order"]
    )

    for name, passed in checks.items():
        if not passed:
            failures.append(name)

    return {
        "schema_version": 1,
        "verdict": "pass" if not failures else "fail",
        "slice": contract["slice"],
        "reference_commit": config["reference"]["commit"],
        "comparison": "5.1-object-semantic-invariants-not-raw-pcm",
        "coordinate_mapping": {
            "aurora": aurora.get("coordinate_convention"),
            "oar": oar.get("coordinate_convention"),
            "mapping": "semantic speaker targets with explicit FL/FR/FC/LFE/SL/SR order",
        },
        "checks": checks,
        "metrics": metrics,
        "failures": failures,
        "truth_boundary": (
            "This proves only the focused 5.1 point-object position/gain and LFE-exclusion semantic slice against the exact pinned OAR reference. "
            "It does not prove IAMF ingestion/decoding, arbitrary between-speaker trajectories, height rendering, 7.1.4 differential equivalence, physical output, or certification."
        ),
    }


def synthetic_probe(implementation: str, perturb: float = 0.0) -> dict[str, Any]:
    cases = []
    definitions = [
        ("front_left_unity", "front_left", -30.0, 0, 1.0),
        ("center_unity", "center", 0.0, 2, 1.0),
        ("front_right_unity", "front_right", 30.0, 1, 1.0),
        ("surround_left_unity", "surround_left", -110.0, 4, 1.0),
        ("surround_right_unity", "surround_right", 110.0, 5, 1.0),
        ("center_minus_6db", "center", 0.0, 2, 10.0 ** (-6.0 / 20.0)),
    ]
    for name, semantic, azimuth, target, amplitude in definitions:
        levels = [0.0] * 6
        levels[target] = amplitude
        if target != 2 and perturb:
            levels[2] = perturb
        cases.append(
            {
                "name": name,
                "semantic_position": semantic,
                "azimuth_degrees": azimuth,
                "gain_db": -6.0 if name == "center_minus_6db" else 0.0,
                "frame_count": 256,
                "channel_levels": levels,
                "finite": True,
            }
        )
    return {
        "schema_version": 1,
        "implementation": implementation,
        "layout": "5.1",
        "sample_rate": 48000,
        "frames_per_case": 256,
        "channel_order": ["FL", "FR", "FC", "LFE", "SL", "SR"],
        "coordinate_convention": "synthetic",
        "lfe_semantics": "non-directional-zero-for-object-render",
        "cases": cases,
    }


def self_test() -> None:
    config = {
        "reference": {"commit": "synthetic-pin"},
        "multichannel_differential": {
            "slice": "five-one-object-position-gain-v1",
            "layout": "5.1",
            "sample_rate": 48000,
            "frames_per_case": 256,
            "channel_order": ["FL", "FR", "FC", "LFE", "SL", "SR"],
            "target_channel_min_fraction": 0.95,
            "normalized_channel_tolerance": 0.05,
            "lfe_relative_max": 1.0e-6,
            "expected_gain_db": -6.0,
            "gain_db_tolerance": 0.05,
            "cross_renderer_gain_db_tolerance": 0.02,
        },
    }
    passing = analyze_documents(
        config, synthetic_probe("aurora"), synthetic_probe("oar", perturb=0.001)
    )
    if passing["verdict"] != "pass":
        raise DifferentialError(f"positive self-test failed: {passing['failures']}")

    leaking = synthetic_probe("oar")
    leaking["cases"][0]["channel_levels"][LFE_INDEX] = 0.1
    failed = analyze_documents(config, synthetic_probe("aurora"), leaking)
    if failed["verdict"] != "fail" or "oar_lfe_silent_front_left_unity" not in failed["failures"]:
        raise DifferentialError("LFE leakage negative self-test did not fail closed")

    swapped = synthetic_probe("oar")
    swapped["channel_order"] = ["FR", "FL", "FC", "LFE", "SL", "SR"]
    try:
        analyze_documents(config, synthetic_probe("aurora"), swapped)
    except DifferentialError:
        pass
    else:
        raise DifferentialError("channel-order negative self-test did not fail closed")

    print("AURORA-OAR-5-1-DIFFERENTIAL-SELFTEST-PASS")


def command_analyze(args: argparse.Namespace) -> int:
    config = load_json(args.config)
    aurora = load_json(args.aurora)
    oar = load_json(args.oar)
    report = analyze_documents(config, aurora, oar)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"verdict={report['verdict']}")
    print(f"report={args.output}")
    if report["verdict"] != "pass":
        print("failures=" + ",".join(report["failures"]), file=sys.stderr)
        return 1
    print("AURORA-OAR-5-1-DIFFERENTIAL-PASS")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("self-test")
    analyze = subparsers.add_parser("analyze")
    analyze.add_argument("--config", type=Path, required=True)
    analyze.add_argument("--aurora", type=Path, required=True)
    analyze.add_argument("--oar", type=Path, required=True)
    analyze.add_argument("--output", type=Path, required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        if args.command == "self-test":
            self_test()
            return 0
        return command_analyze(args)
    except DifferentialError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
