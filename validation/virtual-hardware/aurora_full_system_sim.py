#!/usr/bin/env python3
"""Deterministic virtual hardware sink for Aurora's proven immersive render path.

AuroraSim models the software/hardware boundary after the real Aurora output-DSP
stage. It does not emulate physical electrical behavior. The lab verifies frame
accounting, per-channel identity/order, TDM16 mapping, health limits and explicit
fail-closed fault profiles on a laptop.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
import sys
import tempfile
from array import array
from pathlib import Path
from typing import Any

CHANNELS = 12
SAMPLE_RATE_HZ = 48_000
SAMPLE_BYTES = 4
DEFAULT_TDM_SLOTS = 16
DEFAULT_LATENCY_FRAMES = 256
DEFAULT_MAX_LATENCY_FRAMES = 1024
MAX_HEALTHY_DRIFT_PPM = 100.0
CHUNK_FRAMES = 4096
ZERO_F32 = struct.pack("<f", 0.0)

# This list is consumed by validate_simulation_coverage.py. Adding a new
# full-system capability here without declaring it in the coverage contract
# intentionally breaks CI so simulator coverage cannot silently drift.
SIMULATOR_CAPABILITIES = (
    "output-dsp-chain",
    "render-contract",
    "frame-accounting",
    "channel-activity",
    "channel-order",
    "pcm-identity",
    "tdm16-slot-mapping",
    "xrun-dropout",
    "device-disconnect",
    "clock-drift",
    "sample-rate-integrity",
    "latency-budget",
    "non-finite-protection",
)

FAULT_PROFILES = (
    "none",
    "dropout",
    "channel-silence",
    "channel-swap",
    "disconnect",
    "drift",
    "sample-rate-change",
    "latency-spike",
    "non-finite",
    "tdm-padding-corruption",
)


def _load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object in {path}")
    return payload


def _require_int(mapping: dict[str, Any], key: str) -> int:
    value = mapping.get(key)
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"expected integer {key}, got {value!r}")
    return value


def _evidence_contract(evidence: dict[str, Any]) -> tuple[int, int, int]:
    if evidence.get("verdict") != "pass":
        raise ValueError("Aurora moving-JOC evidence verdict is not pass")
    render = evidence.get("aurora_rendered_7_1_4")
    if not isinstance(render, dict):
        raise ValueError("missing aurora_rendered_7_1_4 evidence")
    channels = _require_int(render, "channels")
    sample_rate = _require_int(render, "sample_rate_hz")
    frames = _require_int(render, "frames")
    if channels != CHANNELS:
        raise ValueError(f"expected {CHANNELS} rendered channels, got {channels}")
    if sample_rate != SAMPLE_RATE_HZ:
        raise ValueError(f"expected {SAMPLE_RATE_HZ} Hz render, got {sample_rate}")
    if frames <= 0:
        raise ValueError("rendered frame count must be positive")
    return channels, sample_rate, frames


def _to_f32_values(raw: bytes) -> array:
    values = array("f")
    values.frombytes(raw)
    if values.itemsize != SAMPLE_BYTES:
        raise RuntimeError("host float size is not 32 bits")
    if sys.byteorder != "little":
        values.byteswap()
    return values


def _to_le_bytes(values: array) -> bytes:
    if sys.byteorder == "little":
        return values.tobytes()
    clone = array("f", values)
    clone.byteswap()
    return clone.tobytes()


def _update_channel_hashes(hashers: list[Any], values: array) -> None:
    for channel in range(CHANNELS):
        hashers[channel].update(_to_le_bytes(values[channel::CHANNELS]))


def _tdm_bytes_from_pcm(
    pcm_bytes: bytes,
    frames: int,
    slots: int,
    *,
    padding_sample: float = 0.0,
) -> bytes:
    if slots < CHANNELS:
        raise ValueError("TDM slot count is smaller than Aurora channel count")
    frame_bytes = CHANNELS * SAMPLE_BYTES
    padding_word = struct.pack("<f", padding_sample)
    padding = padding_word * (slots - CHANNELS)
    if not padding:
        return pcm_bytes
    output = bytearray(frames * slots * SAMPLE_BYTES)
    src = memoryview(pcm_bytes)
    dst_offset = 0
    for frame_index in range(frames):
        src_offset = frame_index * frame_bytes
        output[dst_offset : dst_offset + frame_bytes] = src[
            src_offset : src_offset + frame_bytes
        ]
        dst_offset += frame_bytes
        output[dst_offset : dst_offset + len(padding)] = padding
        dst_offset += len(padding)
    return bytes(output)


def _count_nonzero_tdm_padding(tdm_bytes: bytes, frames: int, slots: int) -> int:
    if slots == CHANNELS:
        return 0
    slot_bytes = slots * SAMPLE_BYTES
    count = 0
    for frame_index in range(frames):
        base = frame_index * slot_bytes
        for slot in range(CHANNELS, slots):
            offset = base + slot * SAMPLE_BYTES
            value = struct.unpack_from("<f", tdm_bytes, offset)[0]
            if value != 0.0:
                count += 1
    return count


def describe() -> dict[str, Any]:
    return {
        "schema_version": 1,
        "model": "aurora-full-system-sim-v2",
        "capabilities": list(SIMULATOR_CAPABILITIES),
        "fault_profiles": list(FAULT_PROFILES),
        "defaults": {
            "sample_rate_hz": SAMPLE_RATE_HZ,
            "channels": CHANNELS,
            "tdm_slots": DEFAULT_TDM_SLOTS,
            "simulated_latency_frames": DEFAULT_LATENCY_FRAMES,
            "simulated_latency_budget_frames": DEFAULT_MAX_LATENCY_FRAMES,
            "maximum_healthy_drift_ppm": MAX_HEALTHY_DRIFT_PPM,
        },
        "truth_boundary": "functional deterministic simulation; not physical/electrical/acoustic evidence",
    }


def simulate(
    render_path: Path,
    evidence_path: Path,
    report_path: Path,
    *,
    fault: str,
    tdm_slots: int,
    latency_frames: int,
    max_latency_frames: int = DEFAULT_MAX_LATENCY_FRAMES,
) -> dict[str, Any]:
    if fault not in FAULT_PROFILES:
        raise ValueError(f"unknown fault profile: {fault}")
    evidence = _load_json(evidence_path)
    _, sample_rate_hz, expected_frames = _evidence_contract(evidence)
    if tdm_slots < CHANNELS:
        raise ValueError(f"tdm-slots must be >= {CHANNELS}")
    if latency_frames < 0:
        raise ValueError("latency-frames must be non-negative")
    if max_latency_frames < 0:
        raise ValueError("max-latency-frames must be non-negative")

    source_sha = hashlib.sha256()
    sink_sha = hashlib.sha256()
    tdm_sha = hashlib.sha256()
    source_channel_sha = [hashlib.sha256() for _ in range(CHANNELS)]
    sink_channel_sha = [hashlib.sha256() for _ in range(CHANNELS)]
    source_frames = 0
    sink_frames = 0
    xrun_count = 0
    disconnect_seen = False
    non_finite_samples = 0
    channel_sum_sq = [0.0] * CHANNELS
    channel_nonzero_samples = [0] * CHANNELS
    dropped_frames = 0
    tdm_padding_nonzero_samples = 0

    dropout_interval = sample_rate_hz
    disconnect_frame = max(1, expected_frames // 2)
    silenced_channel = CHANNELS - 1
    simulated_clock_drift_ppm = 250.0 if fault == "drift" else 0.0
    sink_sample_rate_hz = 44_100 if fault == "sample-rate-change" else sample_rate_hz
    effective_latency_frames = (
        max(latency_frames, max_latency_frames + 512)
        if fault == "latency-spike"
        else latency_frames
    )
    padding_sample = 1.0e-3 if fault == "tdm-padding-corruption" else 0.0

    frame_bytes = CHANNELS * SAMPLE_BYTES
    with render_path.open("rb") as handle:
        while True:
            raw = handle.read(CHUNK_FRAMES * frame_bytes)
            if not raw:
                break
            if len(raw) % frame_bytes:
                raise ValueError(
                    f"render byte count is not aligned to {CHANNELS} f32 channels"
                )
            source_sha.update(raw)
            values = _to_f32_values(raw)
            _update_channel_hashes(source_channel_sha, values)
            chunk_frames = len(values) // CHANNELS
            chunk_start = source_frames
            source_frames += chunk_frames

            output = array("f")
            for local_frame in range(chunk_frames):
                absolute_frame = chunk_start + local_frame
                if fault == "disconnect" and absolute_frame >= disconnect_frame:
                    disconnect_seen = True
                    dropped_frames += chunk_frames - local_frame
                    break
                if (
                    fault == "dropout"
                    and absolute_frame > 0
                    and absolute_frame % dropout_interval == 0
                ):
                    dropped_frames += 1
                    xrun_count += 1
                    continue

                start = local_frame * CHANNELS
                frame = array("f", values[start : start + CHANNELS])
                if fault == "channel-silence":
                    frame[silenced_channel] = 0.0
                elif fault == "channel-swap":
                    frame[0], frame[1] = frame[1], frame[0]
                elif fault == "non-finite" and absolute_frame == 1:
                    frame[0] = float("nan")
                output.extend(frame)

            out_frames = len(output) // CHANNELS
            if out_frames:
                out_bytes = _to_le_bytes(output)
                sink_sha.update(out_bytes)
                _update_channel_hashes(sink_channel_sha, output)
                tdm_bytes = _tdm_bytes_from_pcm(
                    out_bytes,
                    out_frames,
                    tdm_slots,
                    padding_sample=padding_sample,
                )
                tdm_sha.update(tdm_bytes)
                tdm_padding_nonzero_samples += _count_nonzero_tdm_padding(
                    tdm_bytes, out_frames, tdm_slots
                )
                sink_frames += out_frames
                for index, sample in enumerate(output):
                    channel = index % CHANNELS
                    if not math.isfinite(sample):
                        non_finite_samples += 1
                        continue
                    channel_sum_sq[channel] += float(sample) * float(sample)
                    if sample != 0.0:
                        channel_nonzero_samples[channel] += 1
            if disconnect_seen:
                for remaining in iter(
                    lambda: handle.read(CHUNK_FRAMES * frame_bytes), b""
                ):
                    if len(remaining) % frame_bytes:
                        raise ValueError("trailing render bytes are not frame aligned")
                    source_sha.update(remaining)
                    remaining_values = _to_f32_values(remaining)
                    _update_channel_hashes(source_channel_sha, remaining_values)
                    extra = len(remaining) // frame_bytes
                    source_frames += extra
                    dropped_frames += extra
                break

    if source_frames == 0:
        raise ValueError("render contains no frames")

    source_channel_hex = [hasher.hexdigest() for hasher in source_channel_sha]
    sink_channel_hex = [hasher.hexdigest() for hasher in sink_channel_sha]
    per_channel_identity = [
        source_channel_hex[index] == sink_channel_hex[index]
        for index in range(CHANNELS)
    ]
    order_preserved = source_frames == sink_frames and all(per_channel_identity)
    pcm_identity = source_frames == sink_frames and source_sha.hexdigest() == sink_sha.hexdigest()

    rms = [
        math.sqrt(total / sink_frames) if sink_frames > 0 else 0.0
        for total in channel_sum_sq
    ]
    active_channels = [
        index for index, count in enumerate(channel_nonzero_samples) if count > 0
    ]
    inactive_channels = [
        index for index in range(CHANNELS) if index not in active_channels
    ]

    failures: list[str] = []
    if source_frames != expected_frames:
        failures.append(
            f"source_frame_count_mismatch:{source_frames}!={expected_frames}"
        )
    if sink_frames != expected_frames:
        failures.append(f"sink_frame_count_mismatch:{sink_frames}!={expected_frames}")
    if non_finite_samples:
        failures.append(f"non_finite_samples:{non_finite_samples}")
    if inactive_channels:
        failures.append(
            "inactive_output_channels:"
            + ",".join(str(index) for index in inactive_channels)
        )
    if source_frames == sink_frames and not order_preserved:
        failures.append("channel_order_or_identity_mismatch")
    if source_frames == sink_frames and not pcm_identity:
        failures.append("pcm_identity_mismatch")
    if xrun_count:
        failures.append(f"virtual_xruns:{xrun_count}")
    if disconnect_seen:
        failures.append("virtual_device_disconnect")
    if abs(simulated_clock_drift_ppm) > MAX_HEALTHY_DRIFT_PPM:
        failures.append(
            f"simulated_clock_drift_ppm:{simulated_clock_drift_ppm:.3f}"
        )
    if sink_sample_rate_hz != sample_rate_hz:
        failures.append(
            f"sample_rate_mismatch:{sink_sample_rate_hz}!={sample_rate_hz}"
        )
    if effective_latency_frames > max_latency_frames:
        failures.append(
            f"simulated_latency_budget_exceeded:{effective_latency_frames}>{max_latency_frames}"
        )
    if tdm_padding_nonzero_samples:
        failures.append(
            f"tdm_padding_nonzero_samples:{tdm_padding_nonzero_samples}"
        )

    verdict = "pass" if not failures else "fail"
    report: dict[str, Any] = {
        "schema_version": 2,
        "verdict": verdict,
        "fault_profile": fault,
        "coverage_capabilities": list(SIMULATOR_CAPABILITIES),
        "source": {
            "render_path": str(render_path),
            "evidence_path": str(evidence_path),
            "channels": CHANNELS,
            "sample_rate_hz": sample_rate_hz,
            "expected_frames": expected_frames,
            "actual_frames": source_frames,
            "sha256": source_sha.hexdigest(),
            "channel_sha256": source_channel_hex,
        },
        "virtual_hardware": {
            "model": "aurora-virtual-tdm16-dac-v2",
            "synchronous_clock_domain": True,
            "tdm_slots": tdm_slots,
            "assigned_channel_slots": list(range(CHANNELS)),
            "unused_zero_slots": list(range(CHANNELS, tdm_slots)),
            "tdm_padding_nonzero_samples": tdm_padding_nonzero_samples,
            "sink_frames": sink_frames,
            "sink_sample_rate_hz": sink_sample_rate_hz,
            "dropped_frames": dropped_frames,
            "xrun_count": xrun_count,
            "disconnect_seen": disconnect_seen,
            "simulated_clock_drift_ppm": simulated_clock_drift_ppm,
            "simulated_latency_frames": effective_latency_frames,
            "simulated_latency_ms": (effective_latency_frames * 1000.0)
            / sample_rate_hz,
            "simulated_latency_budget_frames": max_latency_frames,
            "simulated_latency_budget_ms": (max_latency_frames * 1000.0)
            / sample_rate_hz,
            "sink_pcm_sha256": sink_sha.hexdigest(),
            "sink_channel_sha256": sink_channel_hex,
            "tdm_stream_sha256": tdm_sha.hexdigest(),
        },
        "channel_health": {
            "active_channel_indices": active_channels,
            "inactive_channel_indices": inactive_channels,
            "nonzero_sample_counts": channel_nonzero_samples,
            "rms": rms,
            "non_finite_samples": non_finite_samples,
            "per_channel_identity": per_channel_identity,
            "order_preserved": order_preserved,
            "pcm_identity_preserved": pcm_identity,
        },
        "failures": failures,
        "truth_boundary": {
            "output_dsp": "real_Aurora_code_when_pipeline_wrapper_is_used",
            "latency": "simulated_not_measured",
            "clock_drift": "simulated_not_measured",
            "sample_rate_fault": "simulated_not_physical_negotiation",
            "electrical_usb_tdm_dac_behavior": "not_evaluated",
            "physical_earc": "not_evaluated",
            "drm_service_compatibility": "not_evaluated",
            "dolby_certification_or_conformance": "not_proven",
            "acoustic_output": "not_evaluated",
        },
    }
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return report


def _fake_evidence(frames: int) -> dict[str, Any]:
    return {
        "verdict": "pass",
        "aurora_rendered_7_1_4": {
            "channels": CHANNELS,
            "sample_rate_hz": SAMPLE_RATE_HZ,
            "frames": frames,
        },
    }


def self_test() -> None:
    frames = SAMPLE_RATE_HZ * 2 + 8
    with tempfile.TemporaryDirectory(prefix="aurora-full-system-sim-") as temp:
        root = Path(temp)
        render = root / "synthetic.f32"
        evidence = root / "evidence.json"
        evidence.write_text(json.dumps(_fake_evidence(frames)), encoding="utf-8")
        values = array("f")
        for frame in range(frames):
            for channel in range(CHANNELS):
                values.append(((frame % 97) + 1) * (channel + 1) * 1.0e-6)
        render.write_bytes(_to_le_bytes(values))

        healthy = simulate(
            render,
            evidence,
            root / "healthy.json",
            fault="none",
            tdm_slots=DEFAULT_TDM_SLOTS,
            latency_frames=DEFAULT_LATENCY_FRAMES,
            max_latency_frames=DEFAULT_MAX_LATENCY_FRAMES,
        )
        if healthy["verdict"] != "pass":
            raise AssertionError(
                f"healthy self-test did not pass: {healthy['failures']}"
            )
        if not healthy["channel_health"]["order_preserved"]:
            raise AssertionError("healthy self-test did not prove channel order")
        if not healthy["channel_health"]["pcm_identity_preserved"]:
            raise AssertionError("healthy self-test did not prove PCM identity")

        for fault in FAULT_PROFILES:
            if fault == "none":
                continue
            result = simulate(
                render,
                evidence,
                root / f"{fault}.json",
                fault=fault,
                tdm_slots=DEFAULT_TDM_SLOTS,
                latency_frames=DEFAULT_LATENCY_FRAMES,
                max_latency_frames=DEFAULT_MAX_LATENCY_FRAMES,
            )
            if result["verdict"] != "fail":
                raise AssertionError(
                    f"fault self-test unexpectedly passed: {fault}"
                )
    print(
        "AURORA-FULL-SYSTEM-SIM-SELF-TEST-PASS "
        f"capabilities={len(SIMULATOR_CAPABILITIES)} negative_profiles={len(FAULT_PROFILES) - 1}"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser(
        "self-test", help="exercise healthy and fail-closed synthetic profiles"
    )
    sub.add_parser("describe", help="emit the simulator capability/fault contract")
    run = sub.add_parser("run", help="analyze a real Aurora 12-channel render")
    run.add_argument("--render", required=True, type=Path)
    run.add_argument("--joc-evidence", required=True, type=Path)
    run.add_argument("--report", required=True, type=Path)
    run.add_argument("--fault", choices=FAULT_PROFILES, default="none")
    run.add_argument("--tdm-slots", type=int, default=DEFAULT_TDM_SLOTS)
    run.add_argument("--latency-frames", type=int, default=DEFAULT_LATENCY_FRAMES)
    run.add_argument(
        "--max-latency-frames", type=int, default=DEFAULT_MAX_LATENCY_FRAMES
    )
    args = parser.parse_args()

    if args.command == "self-test":
        self_test()
        return 0
    if args.command == "describe":
        print(json.dumps(describe(), indent=2, sort_keys=True))
        return 0

    try:
        report = simulate(
            args.render,
            args.joc_evidence,
            args.report,
            fault=args.fault,
            tdm_slots=args.tdm_slots,
            latency_frames=args.latency_frames,
            max_latency_frames=args.max_latency_frames,
        )
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as exc:
        print(f"AURORA-FULL-SYSTEM-SIM-ERROR {exc}", file=sys.stderr)
        return 2

    hw = report["virtual_hardware"]
    channels = report["channel_health"]
    print(
        "AURORA-FULL-SYSTEM-SIM-{} ".format(report["verdict"].upper())
        + f"fault={report['fault_profile']} "
        + f"frames={hw['sink_frames']} "
        + f"channels={len(channels['active_channel_indices'])}/{CHANNELS} "
        + f"tdm_slots={hw['tdm_slots']} "
        + f"xruns={hw['xrun_count']} "
        + f"drift_ppm={hw['simulated_clock_drift_ppm']:.3f} "
        + f"order={channels['order_preserved']} pcm_identity={channels['pcm_identity_preserved']}"
    )
    if report["failures"]:
        print("failures=" + ",".join(report["failures"]))
    print(f"report={args.report}")
    return 0 if report["verdict"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
