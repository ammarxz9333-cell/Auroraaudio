#!/usr/bin/env python3
"""Fail-closed moving-object evidence analysis for Aurora's Harletty/Omniphony path.

This analyzer consumes telemetry emitted by Aurora's validation bridge harness,
a raw 12-channel f32 7.1.4 render, and a pacing report. It deliberately keeps
Aurora-side observations separate from the independent OpenJOC reference lane.
Rendered energy is self-consistency evidence only; it is not an oracle for the
authored object trajectory.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import sys
import tempfile
from array import array
from typing import Any

from joc_temporal_evidence import (
    DEFAULT_ACTIVE_RMS,
    DEFAULT_PROFILE_L1,
    DEFAULT_WINDOW_MS,
    EvidenceError,
    _render_evidence,
)

SCHEMA_VERSION = 1
EXPECTED_CHANNELS = 12
EXPECTED_SAMPLE_RATE = 48_000


def _load_json(path: Path, label: str) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        raise EvidenceError(f"invalid {label} JSON: {exc}") from exc
    if not isinstance(payload, dict) or not payload:
        raise EvidenceError(f"{label} JSON must be a non-empty object")
    return payload


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _position_varies(obj: dict[str, Any]) -> bool:
    minimum = obj.get("position_min")
    maximum = obj.get("position_max")
    if not (
        isinstance(minimum, list)
        and isinstance(maximum, list)
        and len(minimum) == 3
        and len(maximum) == 3
    ):
        return False
    try:
        return any(abs(float(lo) - float(hi)) > 1.0e-9 for lo, hi in zip(minimum, maximum))
    except (TypeError, ValueError):
        return False


def _bridge_evidence(payload: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    transport = payload.get("transport") or {}
    decode = payload.get("decode") or {}
    metadata = payload.get("metadata") or {}
    raw_objects = metadata.get("objects") or []
    if not isinstance(raw_objects, list):
        raw_objects = []

    facts = {
        "source": payload.get("source"),
        "iec61937_data_type": transport.get("data_type"),
        "packets": int(transport.get("packets", 0) or 0),
        "frames": int(decode.get("frames", 0) or 0),
        "sample_rate_hz": int(decode.get("sample_rate_hz", 0) or 0),
        "total_samples": int(decode.get("total_samples", 0) or 0),
        "reset_count": int(decode.get("reset_count", 0) or 0),
        "bridge_ready": decode.get("bridge_ready") is True,
        "bridge_has_objects": decode.get("bridge_has_objects") is True,
        "metadata_frames": int(metadata.get("metadata_frames", 0) or 0),
        "events": int(metadata.get("events", 0) or 0),
        "object_channel_declarations": int(metadata.get("object_channel_declarations", 0) or 0),
        "metadata_sample_positions_monotonic": metadata.get("sample_positions_monotonic") is True,
    }
    bridge_failures: list[str] = []
    requirements = [
        (facts["iec61937_data_type"] == 0x15, "wrong_iec61937_data_type"),
        (facts["packets"] > 0, "no_iec61937_packets"),
        (facts["frames"] > 0, "no_decoded_frames"),
        (facts["sample_rate_hz"] == EXPECTED_SAMPLE_RATE, "unexpected_sample_rate"),
        (facts["total_samples"] > 0, "no_decoded_samples"),
        (facts["reset_count"] == 0, "bridge_reset_during_stream"),
        (facts["bridge_ready"], "bridge_not_ready"),
        (facts["bridge_has_objects"], "bridge_did_not_report_objects"),
        (facts["metadata_frames"] > 0, "no_metadata_frames"),
        (facts["events"] > 0, "no_object_events"),
        (facts["object_channel_declarations"] > 0, "no_object_channel_declarations"),
        (facts["metadata_sample_positions_monotonic"], "metadata_timestamps_not_monotonic"),
    ]
    bridge_failures.extend(reason for ok, reason in requirements if not ok)
    facts["gate_pass"] = not bridge_failures
    facts["failures"] = bridge_failures

    objects: list[dict[str, Any]] = []
    varying_ids: list[int] = []
    change_samples: set[int] = set()
    for raw in raw_objects:
        if not isinstance(raw, dict):
            continue
        object_id = int(raw.get("id", -1))
        varies = _position_varies(raw) and int(raw.get("position_change_count", 0) or 0) > 0
        if varies:
            varying_ids.append(object_id)
        for key in ("first_change_sample", "last_change_sample"):
            value = raw.get(key)
            if isinstance(value, int) and value >= 0:
                change_samples.add(value)
        objects.append(
            {
                "id": object_id,
                "event_count": int(raw.get("event_count", 0) or 0),
                "position_event_count": int(raw.get("position_event_count", 0) or 0),
                "position_change_count": int(raw.get("position_change_count", 0) or 0),
                "first_sample": raw.get("first_sample"),
                "last_sample": raw.get("last_sample"),
                "first_change_sample": raw.get("first_change_sample"),
                "last_change_sample": raw.get("last_change_sample"),
                "position_min": raw.get("position_min"),
                "position_max": raw.get("position_max"),
                "position_varies": varies,
            }
        )

    temporal = {
        "object_count": len(objects),
        "position_varying_object_ids": sorted(varying_ids),
        "distinct_change_samples": sorted(change_samples),
        "objects": objects,
        "temporal_diversity": bool(varying_ids) and facts["events"] > 0,
    }
    return facts, temporal


def _pacing_evidence(payload: dict[str, Any]) -> dict[str, Any]:
    status = payload.get("status")
    evidence = {
        "status": status,
        "media_seconds": payload.get("media_seconds"),
        "elapsed_seconds": payload.get("elapsed_seconds"),
        "realtime_factor": payload.get("realtime_factor"),
        "expected_frames": payload.get("expected_frames"),
        "actual_frames": payload.get("actual_frames"),
        "xrun_marker_count": int(payload.get("xrun_marker_count", 0) or 0),
        "feeder_exit_code": payload.get("feeder_exit_code"),
        "renderer_exit_code": payload.get("renderer_exit_code"),
    }
    evidence["gate_pass"] = (
        status == "pass"
        and evidence["expected_frames"] == evidence["actual_frames"]
        and evidence["xrun_marker_count"] == 0
        and evidence["feeder_exit_code"] == 0
        and evidence["renderer_exit_code"] == 0
    )
    return evidence


def analyze(args: argparse.Namespace) -> tuple[dict[str, Any], int]:
    expected = args.expected_sha256.lower()
    actual = _sha256(args.input)
    if actual != expected:
        raise EvidenceError(f"input SHA-256 mismatch: expected {expected}, got {actual}")
    if not args.provenance.strip():
        raise EvidenceError("provenance must be non-empty")

    telemetry = _load_json(args.telemetry, "Aurora telemetry")
    pacing_payload = _load_json(args.pacing, "pacing")
    bridge, metadata = _bridge_evidence(telemetry)
    rendered = _render_evidence(
        args.pcm,
        sample_rate=args.sample_rate,
        channels=args.channels,
        window_ms=args.window_ms,
        active_rms=args.active_rms,
        profile_l1=args.profile_l1,
    )
    pacing = _pacing_evidence(pacing_payload)

    failures: list[str] = []
    if not bridge["gate_pass"]:
        failures.append("aurora_bridge_gate_failed")
    if not metadata["temporal_diversity"]:
        failures.append("insufficient_aurora_metadata_temporal_diversity")
    if not rendered["temporal_diversity"]:
        failures.append("insufficient_aurora_rendered_temporal_diversity")
    if not pacing["gate_pass"]:
        failures.append("paced_realtime_gate_failed")

    report = {
        "schema_version": SCHEMA_VERSION,
        "verdict": "pass" if not failures else "fail",
        "primary_reason": failures[0] if failures else None,
        "failures": failures,
        "input": {
            "filename": args.input.name,
            "sha256": actual,
            "expected_sha256": expected,
            "provenance": args.provenance,
        },
        "aurora_bridge": bridge,
        "aurora_object_metadata": metadata,
        "aurora_rendered_7_1_4": rendered,
        "pacing_health": pacing,
        "truth_boundary": {
            "metadata_source": "Aurora validation harness observing Harletty bridge_api REvent/RObjectChannel output",
            "render_source": "Omniphony validation renderer fed by the same Harletty bridge and IEC61937 carrier",
            "independent_reference": "OpenJOC evidence remains separate; this report does not relabel it as Aurora evidence",
            "authored_position_correctness": "not_proven",
            "rendered_energy_meaning": "time-windowed 12-channel energy changed during the moving carrier",
            "dolby_conformance_or_certification": "not_proven",
            "physical_hardware_or_drm_streaming": "not_evaluated",
        },
    }
    return report, 0 if not failures else 2


def _write_synthetic_pcm(path: Path) -> None:
    values = array("f")
    for window in range(4):
        lane = window % 2
        for frame in range(4800):
            value = 0.1 * math.sin(2.0 * math.pi * 997.0 * frame / EXPECTED_SAMPLE_RATE)
            for channel in range(EXPECTED_CHANNELS):
                values.append(value if channel == lane else 0.0)
    if sys.byteorder != "little":
        values.byteswap()
    path.write_bytes(values.tobytes())


def self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="aurora-moving-joc-") as root_text:
        root = Path(root_text)
        carrier = root / "carrier.ec3"
        carrier.write_bytes(b"synthetic-only-not-codec-proof")
        expected = _sha256(carrier)
        telemetry = root / "telemetry.json"
        telemetry.write_text(
            json.dumps(
                {
                    "source": "synthetic-self-test",
                    "transport": {"data_type": 0x15, "packets": 4},
                    "decode": {
                        "frames": 4,
                        "sample_rate_hz": 48000,
                        "total_samples": 19200,
                        "reset_count": 0,
                        "bridge_ready": True,
                        "bridge_has_objects": True,
                    },
                    "metadata": {
                        "metadata_frames": 4,
                        "events": 4,
                        "object_channel_declarations": 1,
                        "sample_positions_monotonic": True,
                        "objects": [
                            {
                                "id": 1,
                                "event_count": 4,
                                "position_event_count": 4,
                                "position_change_count": 3,
                                "first_sample": 0,
                                "last_sample": 14400,
                                "first_change_sample": 4800,
                                "last_change_sample": 14400,
                                "position_min": [-1.0, 0.0, 0.0],
                                "position_max": [1.0, 0.0, 0.5],
                            }
                        ],
                    },
                }
            ),
            encoding="utf-8",
        )
        pacing = root / "pacing.json"
        pacing.write_text(
            json.dumps(
                {
                    "status": "pass",
                    "media_seconds": 0.4,
                    "elapsed_seconds": 0.4,
                    "realtime_factor": 1.0,
                    "expected_frames": 19200,
                    "actual_frames": 19200,
                    "xrun_marker_count": 0,
                    "feeder_exit_code": 0,
                    "renderer_exit_code": 0,
                }
            ),
            encoding="utf-8",
        )
        pcm = root / "render.f32"
        _write_synthetic_pcm(pcm)
        args = argparse.Namespace(
            input=carrier,
            expected_sha256=expected,
            provenance="synthetic-self-test",
            telemetry=telemetry,
            pcm=pcm,
            pacing=pacing,
            sample_rate=48000,
            channels=12,
            window_ms=100.0,
            active_rms=DEFAULT_ACTIVE_RMS,
            profile_l1=DEFAULT_PROFILE_L1,
        )
        report, code = analyze(args)
        if code != 0 or report["verdict"] != "pass":
            raise AssertionError("dynamic synthetic Aurora metric fixture should pass")
        print("AURORA-JOC-MOVING-EVIDENCE-SELFTEST-PASS")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    analyze_parser = sub.add_parser("analyze")
    analyze_parser.add_argument("--input", type=Path, required=True)
    analyze_parser.add_argument("--expected-sha256", required=True)
    analyze_parser.add_argument("--provenance", required=True)
    analyze_parser.add_argument("--telemetry", type=Path, required=True)
    analyze_parser.add_argument("--pcm", type=Path, required=True)
    analyze_parser.add_argument("--pacing", type=Path, required=True)
    analyze_parser.add_argument("--sample-rate", type=int, default=EXPECTED_SAMPLE_RATE)
    analyze_parser.add_argument("--channels", type=int, default=EXPECTED_CHANNELS)
    analyze_parser.add_argument("--window-ms", type=float, default=DEFAULT_WINDOW_MS)
    analyze_parser.add_argument("--active-rms", type=float, default=DEFAULT_ACTIVE_RMS)
    analyze_parser.add_argument("--profile-l1", type=float, default=DEFAULT_PROFILE_L1)
    analyze_parser.add_argument("--output", type=Path, required=True)
    sub.add_parser("self-test")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.command == "self-test":
        return self_test()
    try:
        report, code = analyze(args)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(
            "AURORA-JOC-MOVING-EVIDENCE "
            f"verdict={report['verdict']} reason={report['primary_reason'] or 'none'} "
            f"metadata_diversity={str(report['aurora_object_metadata']['temporal_diversity']).lower()} "
            f"rendered_diversity={str(report['aurora_rendered_7_1_4']['temporal_diversity']).lower()} "
            f"pacing={report['pacing_health']['status']}"
        )
        return code
    except EvidenceError as exc:
        print(f"AURORA-JOC-MOVING-EVIDENCE-ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
