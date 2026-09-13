#!/usr/bin/env python3
"""Invariant-based Aurora-vs-OAR differential evidence.

This lane intentionally compares shared spatial semantics rather than raw PCM.
OAR and Aurora use different renderer implementations and opposite azimuth-sign
conventions, so semantic case names and explicit FL/FR ordering are the stable
comparison boundary.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any

REQUIRED_CASES = (
    "left_unity",
    "center_unity",
    "right_unity",
    "center_minus_6db",
)
EXPECTED_SEMANTICS = {
    "left_unity": "left",
    "center_unity": "center",
    "right_unity": "right",
    "center_minus_6db": "center",
}


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
    numeric = float(value)
    if not math.isfinite(numeric):
        raise DifferentialError(f"{label} must be finite")
    return numeric


def case_map(document: dict[str, Any], implementation: str) -> dict[str, dict[str, Any]]:
    cases = document.get("cases")
    if not isinstance(cases, list):
        raise DifferentialError(f"{implementation}: cases must be an array")
    mapped: dict[str, dict[str, Any]] = {}
    for case in cases:
        if not isinstance(case, dict) or not isinstance(case.get("name"), str):
            raise DifferentialError(f"{implementation}: every case needs a string name")
        name = case["name"]
        if name in mapped:
            raise DifferentialError(f"{implementation}: duplicate case {name}")
        mapped[name] = case
    if tuple(mapped.keys()) != REQUIRED_CASES:
        raise DifferentialError(
            f"{implementation}: case order/names must be {list(REQUIRED_CASES)}, got {list(mapped)}"
        )
    return mapped


def validate_probe(document: dict[str, Any], config: dict[str, Any], label: str) -> dict[str, dict[str, Any]]:
    differential = config["differential"]
    if document.get("schema_version") != 1:
        raise DifferentialError(f"{label}: schema_version must be 1")
    if document.get("layout") != differential["layout"]:
        raise DifferentialError(f"{label}: unexpected layout {document.get('layout')!r}")
    if document.get("sample_rate") != differential["sample_rate"]:
        raise DifferentialError(f"{label}: unexpected sample rate {document.get('sample_rate')!r}")
    if document.get("frames_per_case") != differential["frames_per_case"]:
        raise DifferentialError(f"{label}: unexpected frames_per_case {document.get('frames_per_case')!r}")
    if document.get("channel_order") != differential["channel_order"]:
        raise DifferentialError(
            f"{label}: channel order must be {differential['channel_order']}, got {document.get('channel_order')!r}"
        )

    cases = case_map(document, label)
    for name, case in cases.items():
        if case.get("semantic_position") != EXPECTED_SEMANTICS[name]:
            raise DifferentialError(
                f"{label}/{name}: semantic_position must be {EXPECTED_SEMANTICS[name]!r}"
            )
        if case.get("frame_count") != differential["frames_per_case"]:
            raise DifferentialError(f"{label}/{name}: frame accounting mismatch")
        if case.get("finite") is not True:
            raise DifferentialError(f"{label}/{name}: renderer reported non-finite output")
        levels = case.get("channel_levels")
        if not isinstance(levels, list) or len(levels) != 2:
            raise DifferentialError(f"{label}/{name}: channel_levels must contain FL and FR")
        for index, value in enumerate(levels):
            level = finite_number(value, f"{label}/{name}/channel_levels[{index}]")
            if level < 0.0:
                raise DifferentialError(f"{label}/{name}: channel level cannot be negative")
        finite_number(case.get("azimuth_degrees"), f"{label}/{name}/azimuth_degrees")
        finite_number(case.get("gain_db"), f"{label}/{name}/gain_db")
    return cases


def normalize(levels: list[Any], label: str) -> tuple[float, float]:
    left = finite_number(levels[0], f"{label}/FL")
    right = finite_number(levels[1], f"{label}/FR")
    norm = math.hypot(left, right)
    if norm <= 1.0e-12:
        raise DifferentialError(f"{label}: output is effectively silent")
    return left / norm, right / norm


def total_level(case: dict[str, Any], label: str) -> float:
    levels = case["channel_levels"]
    value = math.hypot(
        finite_number(levels[0], f"{label}/FL"),
        finite_number(levels[1], f"{label}/FR"),
    )
    if value <= 1.0e-12:
        raise DifferentialError(f"{label}: total level is effectively silent")
    return value


def gain_delta_db(cases: dict[str, dict[str, Any]], label: str) -> float:
    unity = total_level(cases["center_unity"], f"{label}/center_unity")
    attenuated = total_level(cases["center_minus_6db"], f"{label}/center_minus_6db")
    return 20.0 * math.log10(attenuated / unity)


def analyze_documents(
    config: dict[str, Any], aurora: dict[str, Any], oar: dict[str, Any]
) -> dict[str, Any]:
    differential = config.get("differential")
    if not isinstance(differential, dict):
        raise DifferentialError("config is missing differential contract")

    aurora_cases = validate_probe(aurora, config, "aurora")
    oar_cases = validate_probe(oar, config, "oar")
    failures: list[str] = []
    checks: dict[str, bool] = {}
    metrics: dict[str, Any] = {
        "normalized_channel_delta": {},
        "dominance": {},
    }

    dominance_margin = finite_number(differential["dominance_margin"], "dominance_margin")
    center_tolerance = finite_number(
        differential["center_balance_tolerance"], "center_balance_tolerance"
    )
    normalized_tolerance = finite_number(
        differential["normalized_channel_tolerance"], "normalized_channel_tolerance"
    )

    for implementation, cases in (("aurora", aurora_cases), ("oar", oar_cases)):
        left = normalize(cases["left_unity"]["channel_levels"], f"{implementation}/left")
        center = normalize(cases["center_unity"]["channel_levels"], f"{implementation}/center")
        right = normalize(cases["right_unity"]["channel_levels"], f"{implementation}/right")
        left_margin = left[0] - left[1]
        right_margin = right[1] - right[0]
        center_delta = abs(center[0] - center[1])
        metrics["dominance"][implementation] = {
            "left_fl_minus_fr": left_margin,
            "right_fr_minus_fl": right_margin,
            "center_abs_fl_minus_fr": center_delta,
        }
        checks[f"{implementation}_left_dominance"] = left_margin >= dominance_margin
        checks[f"{implementation}_right_dominance"] = right_margin >= dominance_margin
        checks[f"{implementation}_center_balance"] = center_delta <= center_tolerance

    for name in REQUIRED_CASES:
        aurora_normalized = normalize(aurora_cases[name]["channel_levels"], f"aurora/{name}")
        oar_normalized = normalize(oar_cases[name]["channel_levels"], f"oar/{name}")
        delta = max(
            abs(aurora_normalized[0] - oar_normalized[0]),
            abs(aurora_normalized[1] - oar_normalized[1]),
        )
        metrics["normalized_channel_delta"][name] = delta
        checks[f"normalized_spatial_match_{name}"] = delta <= normalized_tolerance

    target_gain_db = finite_number(differential["expected_gain_db"], "expected_gain_db")
    gain_tolerance = finite_number(differential["gain_db_tolerance"], "gain_db_tolerance")
    cross_gain_tolerance = finite_number(
        differential["cross_renderer_gain_db_tolerance"],
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
        case["frame_count"] == differential["frames_per_case"]
        for cases in (aurora_cases, oar_cases)
        for case in cases.values()
    )
    checks["channel_order"] = (
        aurora["channel_order"] == differential["channel_order"]
        and oar["channel_order"] == differential["channel_order"]
    )

    for name, passed in checks.items():
        if not passed:
            failures.append(name)

    return {
        "schema_version": 1,
        "verdict": "pass" if not failures else "fail",
        "slice": differential["slice"],
        "reference_commit": config["reference"]["commit"],
        "comparison": "semantic-invariants-not-raw-pcm",
        "coordinate_mapping": {
            "aurora": aurora.get("coordinate_convention"),
            "oar": oar.get("coordinate_convention"),
            "mapping": "semantic left/center/right with explicit FL/FR order",
        },
        "checks": checks,
        "metrics": metrics,
        "failures": failures,
        "truth_boundary": (
            "This proves only the focused stereo object-position/gain semantic slice against the exact pinned OAR reference. "
            "It does not prove IAMF ingestion/decoding, 5.1 or 7.1.4 differential equivalence, raw PCM identity, physical output, or certification."
        ),
    }


def synthetic_probe(implementation: str, perturb: float = 0.0) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "implementation": implementation,
        "layout": "stereo",
        "sample_rate": 48000,
        "frames_per_case": 256,
        "channel_order": ["FL", "FR"],
        "coordinate_convention": "synthetic",
        "cases": [
            {
                "name": "left_unity",
                "semantic_position": "left",
                "azimuth_degrees": -45.0,
                "gain_db": 0.0,
                "frame_count": 256,
                "channel_levels": [0.70 - perturb, 0.10 + perturb],
                "finite": True,
            },
            {
                "name": "center_unity",
                "semantic_position": "center",
                "azimuth_degrees": 0.0,
                "gain_db": 0.0,
                "frame_count": 256,
                "channel_levels": [0.50, 0.50],
                "finite": True,
            },
            {
                "name": "right_unity",
                "semantic_position": "right",
                "azimuth_degrees": 45.0,
                "gain_db": 0.0,
                "frame_count": 256,
                "channel_levels": [0.10 + perturb, 0.70 - perturb],
                "finite": True,
            },
            {
                "name": "center_minus_6db",
                "semantic_position": "center",
                "azimuth_degrees": 0.0,
                "gain_db": -6.0,
                "frame_count": 256,
                "channel_levels": [0.2505936, 0.2505936],
                "finite": True,
            },
        ],
    }


def self_test() -> None:
    config = {
        "reference": {"commit": "synthetic-pin"},
        "differential": {
            "slice": "stereo-object-position-gain-v1",
            "layout": "stereo",
            "sample_rate": 48000,
            "frames_per_case": 256,
            "channel_order": ["FL", "FR"],
            "expected_gain_db": -6.0,
            "normalized_channel_tolerance": 0.15,
            "center_balance_tolerance": 0.08,
            "dominance_margin": 0.20,
            "gain_db_tolerance": 0.5,
            "cross_renderer_gain_db_tolerance": 0.25,
        },
    }
    passing = analyze_documents(
        config, synthetic_probe("aurora"), synthetic_probe("oar", perturb=0.01)
    )
    if passing["verdict"] != "pass":
        raise DifferentialError(f"positive self-test failed: {passing['failures']}")

    swapped = synthetic_probe("oar")
    swapped["channel_order"] = ["FR", "FL"]
    try:
        analyze_documents(config, synthetic_probe("aurora"), swapped)
    except DifferentialError:
        pass
    else:
        raise DifferentialError("channel-order negative self-test did not fail closed")

    nonfinite = synthetic_probe("oar")
    nonfinite["cases"][0]["channel_levels"][0] = float("nan")
    try:
        analyze_documents(config, synthetic_probe("aurora"), nonfinite)
    except DifferentialError:
        pass
    else:
        raise DifferentialError("non-finite negative self-test did not fail closed")

    print("AURORA-OAR-DIFFERENTIAL-SELFTEST-PASS")


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
    print("AURORA-OAR-DIFFERENTIAL-PASS")
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
