#!/usr/bin/env python3
"""Fail-closed differential evidence for pinned OAR versus Aurora 2D VBAP."""

from __future__ import annotations

import argparse
import json
import math
import tempfile
from pathlib import Path

EXPECTED_COLUMNS = "azimuth_degrees\tleft_power_share\tright_power_share"


def load_curve(path: Path, expected_magic: str) -> dict[float, tuple[float, float]]:
    lines = path.read_text(encoding="utf-8").splitlines()
    if len(lines) < 3 or lines[0] != expected_magic or lines[1] != EXPECTED_COLUMNS:
        raise ValueError(f"invalid differential TSV header: {path}")

    curve: dict[float, tuple[float, float]] = {}
    for line in lines[2:]:
        fields = line.split("\t")
        if len(fields) != 3:
            raise ValueError(f"invalid differential row: {line!r}")
        azimuth, left, right = map(float, fields)
        if not all(math.isfinite(value) for value in (azimuth, left, right)):
            raise ValueError(f"non-finite differential row: {line!r}")
        if left < 0.0 or right < 0.0:
            raise ValueError(f"negative power share: {line!r}")
        if azimuth in curve:
            raise ValueError(f"duplicate azimuth {azimuth}")
        curve[azimuth] = (left, right)
    return curve


def analyze_curves(
    expected_angles: list[float],
    oar: dict[float, tuple[float, float]],
    aurora: dict[float, tuple[float, float]],
    thresholds: dict[str, float],
) -> dict:
    reasons: list[str] = []
    expected = set(expected_angles)
    if set(oar) != expected:
        reasons.append(f"OAR azimuth set mismatch: got={sorted(oar)} expected={expected_angles}")
    if set(aurora) != expected:
        reasons.append(
            f"Aurora azimuth set mismatch: got={sorted(aurora)} expected={expected_angles}"
        )

    common = sorted(expected.intersection(oar).intersection(aurora))
    power_sum_tolerance = thresholds["power_sum_tolerance"]
    center_balance_max = thresholds["center_balance_max"]
    anchor_dominance_min = thresholds["anchor_dominance_min"]
    mirror_max_abs = thresholds["mirror_max_abs"]
    monotonic_slack = thresholds["monotonic_slack"]
    max_left_share_delta_limit = thresholds["max_left_share_delta"]
    left_share_rmse_limit = thresholds["left_share_rmse"]

    cases = []
    deltas = []
    for angle in common:
        oar_left, oar_right = oar[angle]
        aurora_left, aurora_right = aurora[angle]
        for name, left, right in (
            ("OAR", oar_left, oar_right),
            ("Aurora", aurora_left, aurora_right),
        ):
            power_sum_error = abs((left + right) - 1.0)
            if power_sum_error > power_sum_tolerance:
                reasons.append(
                    f"{name} power shares do not sum to one at {angle:g} deg: error={power_sum_error:.6g}"
                )
            if angle > 0.0 and not left > right:
                reasons.append(f"{name} left/right direction inverted at +{angle:g} deg")
            if angle < 0.0 and not right > left:
                reasons.append(f"{name} left/right direction inverted at {angle:g} deg")
            if abs(angle) >= 30.0:
                dominant = left if angle > 0.0 else right
                if dominant < anchor_dominance_min:
                    reasons.append(
                        f"{name} anchor dominance too weak at {angle:g} deg: {dominant:.6f}"
                    )
            if angle == 0.0 and abs(left - right) > center_balance_max:
                reasons.append(
                    f"{name} center imbalance too large: {abs(left-right):.6f}"
                )

        delta = abs(oar_left - aurora_left)
        deltas.append(delta)
        cases.append(
            {
                "azimuth_degrees": angle,
                "oar": {"left_power_share": oar_left, "right_power_share": oar_right},
                "aurora": {
                    "left_power_share": aurora_left,
                    "right_power_share": aurora_right,
                },
                "left_share_abs_delta": delta,
            }
        )

    def renderer_checks(name: str, curve: dict[float, tuple[float, float]]) -> dict:
        mirror_errors = []
        for angle in sorted(value for value in expected_angles if value > 0.0):
            if angle in curve and -angle in curve:
                pos_left, pos_right = curve[angle]
                neg_left, neg_right = curve[-angle]
                mirror_errors.extend(
                    [abs(pos_left - neg_right), abs(pos_right - neg_left)]
                )
        max_mirror = max(mirror_errors, default=0.0)
        if max_mirror > mirror_max_abs:
            reasons.append(f"{name} mirror symmetry error too large: {max_mirror:.6f}")

        monotonic_violations = []
        ordered = [(angle, curve[angle][0]) for angle in expected_angles if angle in curve]
        for (first_angle, first_left), (second_angle, second_left) in zip(
            ordered, ordered[1:]
        ):
            if second_left + monotonic_slack < first_left:
                monotonic_violations.append(
                    {
                        "from_degrees": first_angle,
                        "to_degrees": second_angle,
                        "from_left_share": first_left,
                        "to_left_share": second_left,
                    }
                )
        if monotonic_violations:
            reasons.append(f"{name} left power is not monotonic with positive-left azimuth")
        return {
            "max_mirror_abs_error": max_mirror,
            "monotonic_violations": monotonic_violations,
        }

    oar_checks = renderer_checks("OAR", oar)
    aurora_checks = renderer_checks("Aurora", aurora)
    max_delta = max(deltas, default=math.inf)
    rmse = math.sqrt(sum(delta * delta for delta in deltas) / len(deltas)) if deltas else math.inf
    if max_delta > max_left_share_delta_limit:
        reasons.append(
            f"maximum OAR/Aurora left-share delta too large: {max_delta:.6f}"
        )
    if rmse > left_share_rmse_limit:
        reasons.append(f"OAR/Aurora left-share RMSE too large: {rmse:.6f}")

    return {
        "verdict": "pass" if not reasons else "fail",
        "reasons": reasons,
        "metrics": {
            "case_count": len(common),
            "max_left_share_abs_delta": max_delta,
            "left_share_rmse": rmse,
            "oar": oar_checks,
            "aurora": aurora_checks,
        },
        "cases": cases,
    }


def run_self_test() -> int:
    angles = [-60.0, -30.0, -15.0, 0.0, 15.0, 30.0, 60.0]
    thresholds = {
        "power_sum_tolerance": 0.001,
        "center_balance_max": 0.10,
        "anchor_dominance_min": 0.90,
        "mirror_max_abs": 0.10,
        "monotonic_slack": 0.02,
        "max_left_share_delta": 0.25,
        "left_share_rmse": 0.15,
    }
    good_values = [0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0]
    good = {angle: (left, 1.0 - left) for angle, left in zip(angles, good_values)}
    perturbed = {
        angle: (min(1.0, max(0.0, left + (0.01 if angle > 0 else -0.01))), 0.0)
        for angle, (left, _) in good.items()
    }
    perturbed = {angle: (left, 1.0 - left) for angle, (left, _) in perturbed.items()}
    if analyze_curves(angles, good, perturbed, thresholds)["verdict"] != "pass":
        raise AssertionError("positive differential self-test did not pass")

    inverted = {angle: (right, left) for angle, (left, right) in good.items()}
    if analyze_curves(angles, good, inverted, thresholds)["verdict"] != "fail":
        raise AssertionError("inverted differential self-test did not fail closed")

    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "curve.tsv"
        path.write_text(
            "OAR_DIFF_V1\n"
            + EXPECTED_COLUMNS
            + "\n0.0\t0.5\t0.5\n",
            encoding="utf-8",
        )
        if load_curve(path, "OAR_DIFF_V1") != {0.0: (0.5, 0.5)}:
            raise AssertionError("TSV parser self-test failed")

    print("OAR-DIFFERENTIAL-ANALYZER-SELF-TEST-PASS")
    return 0


def run_analyze(args: argparse.Namespace) -> int:
    config = json.loads(args.config.read_text(encoding="utf-8"))
    differential = config["differential"]
    expected_angles = [float(value) for value in differential["azimuth_degrees"]]
    thresholds = {key: float(value) for key, value in differential["thresholds"].items()}

    try:
        oar = load_curve(args.oar, "OAR_DIFF_V1")
        aurora = load_curve(args.aurora, "AURORA_DIFF_V1")
        result = analyze_curves(expected_angles, oar, aurora, thresholds)
    except Exception as error:  # Fail closed while still writing evidence.
        result = {
            "verdict": "fail",
            "reasons": [f"differential analyzer error: {error}"],
            "metrics": {},
            "cases": [],
        }

    report = {
        "schema_version": 1,
        "reference": config["reference"],
        "scope": differential["scope"],
        "sample_rate": differential["sample_rate"],
        "block_size": differential["block_size"],
        "output_layout": differential["output_layout"],
        "thresholds": thresholds,
        **result,
        "truth_boundary": (
            "Pinned OAR v1 and Aurora 2D VBAP are compared on identical stereo "
            "point-object azimuth cases using normalized left/right output-power semantics. "
            "This does not prove IAMF bitstream parsing/decoding, 7.1.4 or elevation/HOA "
            "equivalence, acoustic quality, physical output, protected-service behavior, "
            "or certification."
        ),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        f"OAR-DIFFERENTIAL-{report['verdict'].upper()} "
        f"cases={report.get('metrics', {}).get('case_count', 0)} output={args.output}"
    )
    if report["verdict"] != "pass":
        for reason in report["reasons"]:
            print(f"reason: {reason}")
        return 1
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("self-test")
    analyze = subparsers.add_parser("analyze")
    analyze.add_argument("--config", type=Path, required=True)
    analyze.add_argument("--oar", type=Path, required=True)
    analyze.add_argument("--aurora", type=Path, required=True)
    analyze.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    return run_self_test() if args.command == "self-test" else run_analyze(args)


if __name__ == "__main__":
    raise SystemExit(main())
