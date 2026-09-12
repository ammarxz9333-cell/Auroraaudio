#!/usr/bin/env python3
"""Fail-closed temporal evidence analysis for E-AC-3 JOC validation.

This analyzer intentionally separates:
  * OpenJOC codec/JOC/admission/timing facts;
  * timed OAMD/object-state diversity;
  * temporal diversity in an experimental 7.1.4 render; and
  * pacing/health evidence supplied by a separate realtime lane.

Rendered energy is self-consistency evidence only.  It is never treated as an
independent oracle for authored object positions.
"""

from __future__ import annotations

import argparse
from array import array
import hashlib
import json
import math
from pathlib import Path
import re
import sys
import tempfile
from typing import Any

SCHEMA_VERSION = 1
MAX_PCM_BYTES = 512 * 1024 * 1024
MAX_WINDOWS = 4096
DEFAULT_WINDOW_MS = 250.0
DEFAULT_ACTIVE_RMS = 1.0e-5
DEFAULT_PROFILE_L1 = 0.20
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


class EvidenceError(ValueError):
    pass


def _load_json(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:  # deterministic boundary message
        raise EvidenceError(f"invalid inspection JSON: {exc}") from exc
    if not isinstance(payload, dict) or not payload:
        raise EvidenceError("inspection JSON must be a non-empty object")
    return payload


def _codec_evidence(payload: dict[str, Any]) -> dict[str, Any]:
    joc = payload.get("joc") or {}
    eac3 = payload.get("eac3") or {}
    validation = payload.get("validation") or {}
    diagnostics = payload.get("diagnostics") or {}
    compatibility = validation.get("deployed_compatibility") or {}

    facts = {
        "joc_present": joc.get("present") is True,
        "joc_presence_status": joc.get("presence_status"),
        "access_unit_count": int(eac3.get("access_unit_count", 0) or 0),
        "total_samples": int(eac3.get("total_samples", 0) or 0),
        "stream_parse": validation.get("stream_parse"),
        "decoder_admissible": validation.get("decoder_admissible"),
        "frame_timing_continuity": validation.get("frame_timing_continuity"),
        "metadata_timing_continuity": validation.get("metadata_timing_continuity"),
        "deployed_compatibility": compatibility.get("status"),
        "diagnostics_complete": diagnostics.get("complete"),
        "diagnostic_issue_count": int(diagnostics.get("issue_count", 0) or 0),
        "profile_count": len(joc.get("profiles") or []),
    }
    failures: list[str] = []
    requirements = [
        (facts["joc_present"], "joc_not_present"),
        (facts["access_unit_count"] > 0, "no_complete_access_units"),
        (facts["total_samples"] > 0, "no_programme_samples"),
        (facts["stream_parse"] == "pass", "stream_parse_failed"),
        (facts["decoder_admissible"] is True, "decoder_not_admissible"),
        (facts["frame_timing_continuity"] == "continuous", "frame_timing_not_continuous"),
        (facts["metadata_timing_continuity"] == "continuous", "metadata_timing_not_continuous"),
        (facts["deployed_compatibility"] == "pass", "deployed_compatibility_failed"),
        (facts["diagnostics_complete"] is True, "inspection_incomplete"),
        (facts["diagnostic_issue_count"] == 0, "inspection_reported_issues"),
        (facts["profile_count"] > 0, "no_joc_profile"),
    ]
    failures.extend(reason for ok, reason in requirements if not ok)
    facts["gate_pass"] = not failures
    facts["failures"] = failures
    return facts


def _change_sample(value: Any) -> int | None:
    if isinstance(value, dict):
        raw = value.get("sample")
        if isinstance(raw, int) and raw >= 0:
            return raw
    return None


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


def _metadata_evidence(payload: dict[str, Any]) -> dict[str, Any]:
    scene = payload.get("scene") or {}
    objects = scene.get("objects") or []
    if not isinstance(objects, list):
        objects = []

    dynamic_indices: list[int] = []
    varying_indices: list[int] = []
    change_samples: set[int] = set()
    object_summaries: list[dict[str, Any]] = []

    for raw in objects:
        if not isinstance(raw, dict):
            continue
        index = int(raw.get("object_index", len(object_summaries)))
        dynamic = raw.get("dynamic") is True
        varying = _position_varies(raw)
        if dynamic:
            dynamic_indices.append(index)
        if varying:
            varying_indices.append(index)
        first = _change_sample(raw.get("first_change"))
        last = _change_sample(raw.get("last_change"))
        if first is not None:
            change_samples.add(first)
        if last is not None:
            change_samples.add(last)
        object_summaries.append(
            {
                "object_index": index,
                "dynamic": dynamic,
                "metadata_update_count": int(raw.get("metadata_update_count", 0) or 0),
                "position_varies": varying,
                "first_change_sample": first,
                "last_change_sample": last,
                "position_min": raw.get("position_min"),
                "position_max": raw.get("position_max"),
            }
        )

    for key in ("first_change", "last_change"):
        sample = _change_sample(scene.get(key))
        if sample is not None:
            change_samples.add(sample)

    access_units = payload.get("access_units") or []
    au_timestamps = []
    if isinstance(access_units, list):
        for au in access_units:
            if isinstance(au, dict):
                value = au.get("timestamp_seconds")
                if isinstance(value, (int, float)) and math.isfinite(float(value)):
                    au_timestamps.append(float(value))

    update_count = int(scene.get("metadata_update_count", 0) or 0)
    dynamic_detected = scene.get("dynamic_metadata_detected") is True
    diverse = dynamic_detected and update_count > 0 and bool(varying_indices)
    return {
        "metadata_present": scene.get("metadata_present") is True,
        "object_metadata_present": scene.get("object_metadata_present") is True,
        "dynamic_metadata_detected": scene.get("dynamic_metadata_detected"),
        "metadata_update_count": update_count,
        "object_count_reported": len(object_summaries),
        "dynamic_object_indices": sorted(dynamic_indices),
        "position_varying_object_indices": sorted(varying_indices),
        "distinct_change_samples": sorted(change_samples),
        "retained_au_timestamp_count": len(au_timestamps),
        "retained_au_first_timestamp_seconds": au_timestamps[0] if au_timestamps else None,
        "retained_au_last_timestamp_seconds": au_timestamps[-1] if au_timestamps else None,
        "objects": object_summaries,
        "temporal_diversity": diverse,
    }


def _read_pcm(path: Path) -> array:
    size = path.stat().st_size
    if size <= 0:
        raise EvidenceError("rendered PCM is empty")
    if size > MAX_PCM_BYTES:
        raise EvidenceError(f"rendered PCM exceeds {MAX_PCM_BYTES} byte analysis limit")
    if size % 4:
        raise EvidenceError("rendered PCM is not whole f32 samples")
    values = array("f")
    values.frombytes(path.read_bytes())
    if sys.byteorder != "little":
        values.byteswap()
    if not all(math.isfinite(value) for value in values):
        raise EvidenceError("rendered PCM contains NaN/Inf")
    return values


def _render_evidence(
    pcm_path: Path,
    *,
    sample_rate: int,
    channels: int,
    window_ms: float,
    active_rms: float,
    profile_l1: float,
) -> dict[str, Any]:
    if sample_rate <= 0 or channels <= 0:
        raise EvidenceError("sample rate and channel count must be positive")
    if channels != 12:
        raise EvidenceError(f"temporal 7.1.4 evidence requires 12 channels, got {channels}")
    if not (window_ms > 0 and math.isfinite(window_ms)):
        raise EvidenceError("window_ms must be finite and positive")
    if active_rms < 0 or profile_l1 < 0:
        raise EvidenceError("thresholds must be non-negative")

    values = _read_pcm(pcm_path)
    if len(values) % channels:
        raise EvidenceError("rendered PCM is not whole multichannel frames")
    frames = len(values) // channels
    window_frames = max(1, round(sample_rate * window_ms / 1000.0))
    window_count = (frames + window_frames - 1) // window_frames
    if window_count > MAX_WINDOWS:
        raise EvidenceError(f"window count {window_count} exceeds {MAX_WINDOWS}")

    windows: list[dict[str, Any]] = []
    normalized_profiles: list[list[float]] = []
    active_sets: set[tuple[int, ...]] = set()

    for window_index in range(window_count):
        first_frame = window_index * window_frames
        last_frame = min(frames, first_frame + window_frames)
        count = last_frame - first_frame
        sums = [0.0] * channels
        peaks = [0.0] * channels
        for frame in range(first_frame, last_frame):
            base = frame * channels
            for channel in range(channels):
                value = float(values[base + channel])
                sums[channel] += value * value
                peaks[channel] = max(peaks[channel], abs(value))
        rms = [math.sqrt(total / count) if count else 0.0 for total in sums]
        active = tuple(index for index, value in enumerate(rms) if value >= active_rms)
        if active:
            active_sets.add(active)
        energy_total = sum(value * value for value in rms)
        normalized = (
            [(value * value) / energy_total for value in rms]
            if energy_total > 0.0
            else [0.0] * channels
        )
        if active:
            normalized_profiles.append(normalized)
        windows.append(
            {
                "index": window_index,
                "start_frame": first_frame,
                "end_frame": last_frame,
                "start_seconds": first_frame / sample_rate,
                "end_seconds": last_frame / sample_rate,
                "rms": rms,
                "peak": peaks,
                "active_lanes": list(active),
                "normalized_energy": normalized,
            }
        )

    max_l1 = 0.0
    if normalized_profiles:
        baseline = normalized_profiles[0]
        for profile in normalized_profiles[1:]:
            max_l1 = max(max_l1, sum(abs(a - b) for a, b in zip(baseline, profile)))

    diverse = len(active_sets) >= 2 or max_l1 >= profile_l1
    return {
        "channels": channels,
        "sample_rate_hz": sample_rate,
        "frames": frames,
        "duration_seconds": frames / sample_rate,
        "window_ms": window_ms,
        "window_count": window_count,
        "non_silent_window_count": len(normalized_profiles),
        "active_rms_threshold": active_rms,
        "profile_l1_threshold": profile_l1,
        "distinct_active_lane_sets": [list(value) for value in sorted(active_sets)],
        "max_normalized_profile_l1_from_first": max_l1,
        "temporal_diversity": diverse,
        "windows": windows,
    }


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def analyze(args: argparse.Namespace) -> tuple[dict[str, Any], int]:
    expected = args.expected_sha256.lower()
    if not SHA256_RE.fullmatch(expected):
        raise EvidenceError("expected SHA-256 must be exactly 64 lowercase/uppercase hex digits")
    actual = _sha256(args.input)
    if actual != expected:
        raise EvidenceError(f"input SHA-256 mismatch: expected {expected}, got {actual}")
    if not args.provenance.strip():
        raise EvidenceError("provenance must be non-empty")

    inspect = _load_json(args.inspect)
    codec = _codec_evidence(inspect)
    metadata = _metadata_evidence(inspect)
    rendered = _render_evidence(
        args.pcm,
        sample_rate=args.sample_rate,
        channels=args.channels,
        window_ms=args.window_ms,
        active_rms=args.active_rms,
        profile_l1=args.profile_l1,
    )

    failures: list[str] = []
    if not codec["gate_pass"]:
        failures.append("codec_or_joc_gate_failed")
    if not metadata["temporal_diversity"]:
        failures.append("insufficient_metadata_temporal_diversity")
    if not rendered["temporal_diversity"]:
        failures.append("insufficient_rendered_temporal_diversity")

    if failures:
        if all(reason.startswith("insufficient_") for reason in failures):
            primary_reason = "insufficient_temporal_diversity"
            exit_code = 3
        else:
            primary_reason = failures[0]
            exit_code = 2
        verdict = "fail"
    else:
        primary_reason = None
        exit_code = 0
        verdict = "pass"

    report = {
        "schema_version": SCHEMA_VERSION,
        "verdict": verdict,
        "primary_reason": primary_reason,
        "failures": failures,
        "input": {
            "filename": args.input.name,
            "sha256": actual,
            "expected_sha256": expected,
            "provenance": args.provenance,
        },
        "codec_joc": codec,
        "object_metadata": metadata,
        "rendered_7_1_4": rendered,
        "pacing_health": {
            "status": args.pacing_status,
            "evidence": args.pacing_evidence,
        },
        "truth_boundary": {
            "rendered_temporal_diversity_means": (
                "time-windowed channel-energy profiles changed in the experimental 7.1.4 render"
            ),
            "rendered_temporal_diversity_does_not_mean": (
                "rendered channel energy independently proves authored object positions or trajectories"
            ),
            "authored_position_correctness": "not_proven_by_this_harness",
            "physical_hardware_or_drm_streaming": "not_evaluated",
            "synthetic_self_tests": "metric/report logic only; never codec/JOC proof",
        },
    }
    return report, exit_code


def _write_report(report: dict[str, Any], output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _synthetic_inspection(dynamic: bool) -> dict[str, Any]:
    changing = {
        "object_index": 0,
        "first_active_au": 0,
        "last_active_au": 3,
        "metadata_update_count": 3 if dynamic else 0,
        "dynamic": dynamic,
        "first_change": {"au": 1, "sample": 4800, "seconds": 0.1} if dynamic else None,
        "last_change": {"au": 2, "sample": 9600, "seconds": 0.2} if dynamic else None,
        "position_min": [-1.0, 0.0, 0.0],
        "position_max": [1.0, 0.0, 0.0] if dynamic else [-1.0, 0.0, 0.0],
    }
    return {
        "joc": {"present": True, "presence_status": "present", "profiles": [{"value": {}}]},
        "eac3": {"access_unit_count": 4, "total_samples": 19200},
        "validation": {
            "stream_parse": "pass",
            "decoder_admissible": True,
            "frame_timing_continuity": "continuous",
            "metadata_timing_continuity": "continuous",
            "deployed_compatibility": {"status": "pass"},
        },
        "diagnostics": {"complete": True, "issue_count": 0},
        "scene": {
            "metadata_present": True,
            "object_metadata_present": True,
            "dynamic_metadata_detected": dynamic,
            "metadata_update_count": 3 if dynamic else 0,
            "first_change": changing["first_change"],
            "last_change": changing["last_change"],
            "objects": [changing],
        },
        "access_units": [
            {"au": index, "timestamp_seconds": index * 0.1} for index in range(4)
        ],
    }


def _write_synthetic_pcm(path: Path, dynamic: bool) -> None:
    channels = 12
    sample_rate = 48000
    window_frames = 4800
    values = array("f")
    for window in range(4):
        lane = (window % 2) if dynamic else 0
        for frame in range(window_frames):
            phase = 2.0 * math.pi * 997.0 * frame / sample_rate
            sample = 0.1 * math.sin(phase)
            for channel in range(channels):
                values.append(sample if channel == lane else 0.0)
    if sys.byteorder != "little":
        values.byteswap()
    path.write_bytes(values.tobytes())


def self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="aurora-joc-temporal-") as root_text:
        root = Path(root_text)
        input_path = root / "carrier.eac3"
        input_path.write_bytes(b"synthetic-not-a-codec-proof")
        expected = _sha256(input_path)

        for dynamic, expected_code in ((True, 0), (False, 3)):
            inspect = root / f"inspect-{dynamic}.json"
            pcm = root / f"render-{dynamic}.f32"
            inspect.write_text(json.dumps(_synthetic_inspection(dynamic)), encoding="utf-8")
            _write_synthetic_pcm(pcm, dynamic)
            ns = argparse.Namespace(
                input=input_path,
                expected_sha256=expected,
                provenance="synthetic-self-test",
                inspect=inspect,
                pcm=pcm,
                sample_rate=48000,
                channels=12,
                window_ms=100.0,
                active_rms=DEFAULT_ACTIVE_RMS,
                profile_l1=DEFAULT_PROFILE_L1,
                pacing_status="not_evaluated",
                pacing_evidence="synthetic-self-test",
            )
            report, code = analyze(ns)
            if code != expected_code:
                raise AssertionError(f"dynamic={dynamic}: expected exit {expected_code}, got {code}")
            if dynamic and report["verdict"] != "pass":
                raise AssertionError("dynamic synthetic metric fixture should pass analyzer logic")
            if not dynamic and report["primary_reason"] != "insufficient_temporal_diversity":
                raise AssertionError("static synthetic metric fixture must fail closed on temporal diversity")

        print("JOC-TEMPORAL-ANALYZER-SELFTEST-PASS")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    analyze_parser = sub.add_parser("analyze")
    analyze_parser.add_argument("--input", type=Path, required=True)
    analyze_parser.add_argument("--expected-sha256", required=True)
    analyze_parser.add_argument("--provenance", required=True)
    analyze_parser.add_argument("--inspect", type=Path, required=True)
    analyze_parser.add_argument("--pcm", type=Path, required=True)
    analyze_parser.add_argument("--sample-rate", type=int, required=True)
    analyze_parser.add_argument("--channels", type=int, default=12)
    analyze_parser.add_argument("--window-ms", type=float, default=DEFAULT_WINDOW_MS)
    analyze_parser.add_argument("--active-rms", type=float, default=DEFAULT_ACTIVE_RMS)
    analyze_parser.add_argument("--profile-l1", type=float, default=DEFAULT_PROFILE_L1)
    analyze_parser.add_argument(
        "--pacing-status",
        choices=("pass", "fail", "not_evaluated"),
        default="not_evaluated",
    )
    analyze_parser.add_argument("--pacing-evidence", default="not supplied")
    analyze_parser.add_argument("--output", type=Path, required=True)

    sub.add_parser("self-test")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.command == "self-test":
        return self_test()
    try:
        report, code = analyze(args)
        _write_report(report, args.output)
        print(
            "JOC-TEMPORAL-EVIDENCE "
            f"verdict={report['verdict']} reason={report['primary_reason'] or 'none'} "
            f"metadata_diversity={str(report['object_metadata']['temporal_diversity']).lower()} "
            f"rendered_diversity={str(report['rendered_7_1_4']['temporal_diversity']).lower()}"
        )
        return code
    except EvidenceError as exc:
        print(f"JOC-TEMPORAL-EVIDENCE-FAIL: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
