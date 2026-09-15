#!/usr/bin/env python3
"""Deterministic Aurora-vs-OBR 7.1.4 binaural semantic differential."""

from __future__ import annotations

import argparse
import array
import json
import math
import subprocess
import sys
import wave
from pathlib import Path
from typing import Any

CANONICAL_ORDER = [
    "FL",
    "FR",
    "FC",
    "LFE",
    "SL",
    "SR",
    "SBL",
    "SBR",
    "TFL",
    "TFR",
    "TRL",
    "TRR",
]

CANONICAL_SIDES = {
    "FL": "left",
    "FR": "right",
    "FC": "center",
    "LFE": "lfe",
    "SL": "left",
    "SR": "right",
    "SBL": "left",
    "SBR": "right",
    "TFL": "left",
    "TFR": "right",
    "TRL": "left",
    "TRR": "right",
}


def git_head(path: Path) -> str:
    return subprocess.check_output(
        ["git", "-C", str(path), "rev-parse", "HEAD"], text=True
    ).strip()


def finite_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and math.isfinite(float(value))


def write_channel_impulse(
    path: Path,
    channel_index: int,
    channels: int,
    frames: int,
    impulse_frame: int,
    amplitude: int,
    sample_rate: int,
) -> None:
    samples = array.array("h", [0]) * (channels * frames)
    samples[impulse_frame * channels + channel_index] = amplitude
    if sys.byteorder != "little":
        samples.byteswap()
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(channels)
        handle.setsampwidth(2)
        handle.setframerate(sample_rate)
        handle.writeframes(samples.tobytes())


def read_stereo_metrics(path: Path) -> dict[str, Any]:
    with wave.open(str(path), "rb") as handle:
        channels = handle.getnchannels()
        sample_width = handle.getsampwidth()
        sample_rate = handle.getframerate()
        frame_count = handle.getnframes()
        raw = handle.readframes(frame_count)
    if sample_width != 2:
        raise ValueError(f"expected 16-bit OBR output, got {sample_width * 8}-bit")
    samples = array.array("h")
    samples.frombytes(raw)
    if sys.byteorder != "little":
        samples.byteswap()
    if channels != 2:
        return {
            "channels": channels,
            "sample_width_bits": sample_width * 8,
            "sample_rate_hz": sample_rate,
            "frames": frame_count,
            "left_energy": 0.0,
            "right_energy": 0.0,
            "total_energy": 0.0,
            "left_right_power_bias_db": float("nan"),
        }
    left_energy = 0.0
    right_energy = 0.0
    for index in range(0, len(samples), 2):
        left = float(samples[index])
        right = float(samples[index + 1])
        left_energy += left * left
        right_energy += right * right
    epsilon = 1.0e-12
    bias_db = 10.0 * math.log10((left_energy + epsilon) / (right_energy + epsilon))
    return {
        "channels": channels,
        "sample_width_bits": sample_width * 8,
        "sample_rate_hz": sample_rate,
        "frames": frame_count,
        "left_energy": left_energy,
        "right_energy": right_energy,
        "total_energy": left_energy + right_energy,
        "left_right_power_bias_db": bias_db,
    }


def directional_pass(
    expected_side: str,
    bias_db: float,
    min_spatial_bias_db: float,
    center_max_abs_bias_db: float,
) -> bool:
    if expected_side == "left":
        return bias_db >= min_spatial_bias_db
    if expected_side == "right":
        return bias_db <= -min_spatial_bias_db
    if expected_side == "center":
        return abs(bias_db) <= center_max_abs_bias_db
    if expected_side == "lfe":
        return True
    return False


def require_contract_shape(config: dict[str, Any]) -> None:
    obr = config.get("google_obr", {})
    for field, supported in {
        "input_type": "7.1.4",
        "filter_type": "Direct",
        "cli_target": "//obr/cli:obr_cli",
    }.items():
        if obr.get(field) != supported:
            raise SystemExit(f"unsupported google_obr.{field}: expected {supported}")
    if config.get("schema_version") != 1 or config.get("roadmap_phase") != 11:
        raise SystemExit("unsupported binaural 7.1.4 differential contract")
    fixture = config.get("fixture", {})
    if fixture.get("channel_order") != CANONICAL_ORDER:
        raise SystemExit("7.1.4 channel order drifted from canonical sequential OBR/Aurora order")
    if config.get("directional_cases") != CANONICAL_SIDES:
        raise SystemExit("7.1.4 directional case map drifted from canonical contract")
    if fixture.get("sample_rate_hz") != 48_000 or fixture.get("sample_width_bits") != 16:
        raise SystemExit("OBR differential requires deterministic 16-bit/48 kHz fixtures")
    if fixture.get("frames", 0) <= 0 or fixture.get("processing_buffer_frames", 0) <= 0:
        raise SystemExit("invalid deterministic fixture sizing")
    if fixture["frames"] % fixture["processing_buffer_frames"] != 0:
        raise SystemExit("fixture frame count must be a whole number of OBR processing buffers")
    if not 0 <= fixture.get("impulse_frame", -1) < fixture["frames"]:
        raise SystemExit("impulse frame outside fixture")


def validate_aurora_probe(
    probe: dict[str, Any], config: dict[str, Any]
) -> tuple[dict[str, dict[str, Any]], list[dict[str, Any]]]:
    if probe.get("schema_version") != 1:
        raise SystemExit("unsupported Aurora binaural probe schema")
    if probe.get("channel_order") != CANONICAL_ORDER:
        raise SystemExit("Aurora probe channel order does not match canonical 7.1.4 order")
    if probe.get("sample_rate_hz") != config["fixture"]["sample_rate_hz"]:
        raise SystemExit("Aurora probe sample rate does not match differential contract")

    by_role: dict[str, dict[str, Any]] = {}
    checks: list[dict[str, Any]] = []
    for case in probe.get("cases", []):
        role = case.get("role")
        if role in by_role:
            raise SystemExit(f"duplicate Aurora probe role: {role}")
        by_role[role] = case
    if set(by_role) != set(CANONICAL_ORDER):
        raise SystemExit("Aurora probe did not emit exactly the canonical 7.1.4 roles")

    thresholds = config["thresholds"]
    for role in CANONICAL_ORDER:
        case = by_role[role]
        bias_db = case.get("left_right_power_bias_db")
        numeric_fields = [
            case.get("left_gain"),
            case.get("right_gain"),
            case.get("left_delay_samples"),
            case.get("right_delay_samples"),
            bias_db,
        ]
        finite = bool(case.get("finite")) and all(finite_number(value) for value in numeric_fields)
        expected_side = CANONICAL_SIDES[role]
        side_ok = finite and directional_pass(
            expected_side,
            float(bias_db),
            float(thresholds["aurora_min_spatial_bias_db"]),
            float(thresholds["aurora_center_max_abs_bias_db"]),
        )
        checks.append(
            {
                "role": role,
                "expected_side": expected_side,
                "bias_db": bias_db,
                "finite": finite,
                "directional_semantics_pass": side_ok,
                "pass": finite and side_ok,
            }
        )
    return by_role, checks


def run_obr_cases(
    config: dict[str, Any], obr_cli: Path, work_dir: Path
) -> dict[str, dict[str, Any]]:
    fixture = config["fixture"]
    work_dir.mkdir(parents=True, exist_ok=True)
    metrics: dict[str, dict[str, Any]] = {}
    for channel_index, role in enumerate(CANONICAL_ORDER):
        input_path = work_dir / f"{channel_index:02d}-{role}-input.wav"
        output_path = work_dir / f"{channel_index:02d}-{role}-obr.wav"
        write_channel_impulse(
            input_path,
            channel_index,
            len(CANONICAL_ORDER),
            int(fixture["frames"]),
            int(fixture["impulse_frame"]),
            int(fixture["impulse_amplitude"]),
            int(fixture["sample_rate_hz"]),
        )
        command = [
            str(obr_cli),
            "--input_type=7.1.4",
            f"--input_file={input_path}",
            f"--output_file={output_path}",
            f"--buffer_size={fixture['processing_buffer_frames']}",
            "--filter_type=Direct",
        ]
        completed = subprocess.run(command, text=True, capture_output=True)
        if completed.returncode != 0:
            raise RuntimeError(
                f"OBR failed for {role}:\nSTDOUT:\n{completed.stdout}\nSTDERR:\n{completed.stderr}"
            )
        role_metrics = read_stereo_metrics(output_path)
        role_metrics["role"] = role
        role_metrics["channel_index"] = channel_index
        metrics[role] = role_metrics
    return metrics


def evaluate_obr(
    metrics: dict[str, dict[str, Any]], config: dict[str, Any]
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    fixture = config["fixture"]
    thresholds = config["thresholds"]
    case_checks: list[dict[str, Any]] = []
    for role in CANONICAL_ORDER:
        item = metrics[role]
        bias_db = float(item["left_right_power_bias_db"])
        expected_side = CANONICAL_SIDES[role]
        finite = all(
            finite_number(item[key])
            for key in ["left_energy", "right_energy", "total_energy", "left_right_power_bias_db"]
        )
        format_ok = (
            item["channels"] == 2
            and item["sample_width_bits"] == 16
            and item["sample_rate_hz"] == fixture["sample_rate_hz"]
            and item["frames"] == fixture["frames"]
        )
        nonzero_ok = expected_side == "lfe" or item["total_energy"] > 0.0
        directional_ok = finite and directional_pass(
            expected_side,
            bias_db,
            float(thresholds["obr_min_spatial_bias_db"]),
            float(thresholds["obr_center_max_abs_bias_db"]),
        )
        case_checks.append(
            {
                "role": role,
                "expected_side": expected_side,
                "bias_db": bias_db,
                "output_format_pass": format_ok,
                "finite": finite,
                "nonzero_spatial_output": nonzero_ok,
                "directional_semantics_pass": directional_ok,
                "pass": format_ok and finite and nonzero_ok and directional_ok,
            }
        )

    mirror_checks: list[dict[str, Any]] = []
    for left_role, right_role in config["mirror_pairs"]:
        left_bias = float(metrics[left_role]["left_right_power_bias_db"])
        right_bias = float(metrics[right_role]["left_right_power_bias_db"])
        mirror_error = abs(left_bias + right_bias)
        passed = mirror_error <= float(thresholds["obr_mirror_max_sum_abs_db"])
        mirror_checks.append(
            {
                "left_role": left_role,
                "right_role": right_role,
                "left_bias_db": left_bias,
                "right_bias_db": right_bias,
                "mirror_sum_abs_db": mirror_error,
                "pass": passed,
            }
        )
    return case_checks, mirror_checks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--obr-root", type=Path, required=True)
    parser.add_argument("--obr-cli", type=Path, required=True)
    parser.add_argument("--aurora-json", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    config = json.loads(args.config.read_text(encoding="utf-8"))
    require_contract_shape(config)
    actual_obr_head = git_head(args.obr_root)
    expected_obr_head = config["google_obr"]["pinned_commit"]
    if actual_obr_head != expected_obr_head:
        raise SystemExit(
            f"OBR pin mismatch: expected {expected_obr_head}, got {actual_obr_head}"
        )
    if not args.obr_cli.is_file():
        raise SystemExit(f"OBR CLI missing: {args.obr_cli}")

    aurora_probe = json.loads(args.aurora_json.read_text(encoding="utf-8"))
    _, aurora_checks = validate_aurora_probe(aurora_probe, config)
    obr_metrics = run_obr_cases(config, args.obr_cli, args.work_dir)
    obr_checks, mirror_checks = evaluate_obr(obr_metrics, config)

    aurora_by_role = {item["role"]: item for item in aurora_checks}
    obr_by_role = {item["role"]: item for item in obr_checks}
    differential_checks: list[dict[str, Any]] = []
    for role in CANONICAL_ORDER:
        if role == "LFE":
            semantic_match = True
        else:
            semantic_match = (
                aurora_by_role[role]["directional_semantics_pass"]
                and obr_by_role[role]["directional_semantics_pass"]
            )
        differential_checks.append(
            {
                "role": role,
                "expected_side": CANONICAL_SIDES[role],
                "aurora_bias_db": aurora_by_role[role]["bias_db"],
                "obr_bias_db": obr_by_role[role]["bias_db"],
                "semantic_match": semantic_match,
                "pass": semantic_match,
            }
        )

    verdict = (
        all(item["pass"] for item in aurora_checks)
        and all(item["pass"] for item in obr_checks)
        and all(item["pass"] for item in mirror_checks)
        and all(item["pass"] for item in differential_checks)
    )
    evidence = {
        "schema_version": 1,
        "roadmap_phase": 11,
        "evidence_class": "software_reference_differential",
        "contract": str(args.config),
        "obr": {
            "expected_commit": expected_obr_head,
            "actual_commit": actual_obr_head,
            "cli": str(args.obr_cli),
            "filter_type": config["google_obr"]["filter_type"],
            "case_metrics": obr_metrics,
            "case_checks": obr_checks,
            "mirror_checks": mirror_checks,
        },
        "aurora": {
            "renderer": aurora_probe.get("renderer"),
            "probe_truth_boundary": aurora_probe.get("truth_boundary"),
            "case_checks": aurora_checks,
        },
        "differential_checks": differential_checks,
        "channel_order": CANONICAL_ORDER,
        "channel_order_boundary": config["fixture"]["channel_order_semantics"],
        "truth_boundary": config["truth_boundary"],
        "verdict": "pass" if verdict else "fail",
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    print(json.dumps(evidence, indent=2, sort_keys=True))
    return 0 if verdict else 1


if __name__ == "__main__":
    sys.exit(main())
