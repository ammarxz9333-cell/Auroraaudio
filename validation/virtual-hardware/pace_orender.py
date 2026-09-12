#!/usr/bin/env python3
"""Feed a fixed-burst IEC61937 carrier to orender at media cadence.

Designed for the Windows AuroraSim launcher but intentionally cross-platform.
It records pacing health separately from decoder/render correctness.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys
import time

SYNC = bytes.fromhex("72f81f4e")
CHANNELS = 12
SAMPLE_RATE = 48_000
SAMPLE_BYTES = 4


def burst_geometry(carrier: bytes) -> tuple[int, int]:
    if not carrier.startswith(SYNC):
        raise ValueError("IEC61937 carrier does not start with sync")
    second = carrier.find(SYNC, 4)
    if second <= 0:
        raise ValueError("IEC61937 carrier contains fewer than two bursts")
    if len(carrier) % second:
        raise ValueError(
            f"carrier length {len(carrier)} is not a multiple of burst size {second}"
        )
    bursts = len(carrier) // second
    for index in range(bursts):
        offset = index * second
        if carrier[offset : offset + 4] != SYNC:
            raise ValueError(f"IEC61937 sync missing at burst {index}")
        if carrier[offset + 4] & 0x1F != 0x15:
            raise ValueError(f"non-E-AC-3 IEC61937 data type at burst {index}")
    return second, bursts


def frame_count(path: pathlib.Path) -> int:
    size = path.stat().st_size
    frame_bytes = CHANNELS * SAMPLE_BYTES
    if size % frame_bytes:
        raise ValueError(f"{path} is not whole {CHANNELS}-channel f32 frames")
    return size // frame_bytes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--orender", required=True, type=pathlib.Path)
    parser.add_argument("--bridge", required=True, type=pathlib.Path)
    parser.add_argument("--layout", required=True, type=pathlib.Path)
    parser.add_argument("--carrier", required=True, type=pathlib.Path)
    parser.add_argument("--unpaced-render", required=True, type=pathlib.Path)
    parser.add_argument("--paced-render", required=True, type=pathlib.Path)
    parser.add_argument("--log", required=True, type=pathlib.Path)
    parser.add_argument("--report", required=True, type=pathlib.Path)
    parser.add_argument("--timeout-seconds", type=float, default=110.0)
    args = parser.parse_args()

    carrier = args.carrier.read_bytes()
    burst_bytes, bursts = burst_geometry(carrier)
    expected_frames = frame_count(args.unpaced_render)
    if expected_frames <= 0 or expected_frames % bursts:
        raise SystemExit(
            f"cannot derive integral media cadence: frames={expected_frames} bursts={bursts}"
        )
    frames_per_burst = expected_frames // bursts
    if frames_per_burst != 1536:
        raise SystemExit(
            f"expected 1536 rendered frames per E-AC-3 burst, got {frames_per_burst}"
        )
    interval = frames_per_burst / SAMPLE_RATE
    media_seconds = expected_frames / SAMPLE_RATE

    args.paced_render.parent.mkdir(parents=True, exist_ok=True)
    args.log.parent.mkdir(parents=True, exist_ok=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)

    command = [
        str(args.orender),
        "-",
        "--bridge-path",
        str(args.bridge),
        "--enable-vbap",
        "--speaker-layout",
        str(args.layout),
        "--output-backend",
        "file",
        "--output-file",
        str(args.paced_render),
        "--output-file-format",
        "raw-f32",
    ]

    start = time.monotonic()
    feeder_rc = 0
    timed_out = False
    broken_pipe = False
    with args.log.open("wb") as log_handle:
        process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=log_handle,
            stderr=subprocess.STDOUT,
        )
        assert process.stdin is not None
        deadline = time.monotonic()
        try:
            for burst_index in range(bursts):
                offset = burst_index * burst_bytes
                process.stdin.write(carrier[offset : offset + burst_bytes])
                process.stdin.flush()
                deadline += interval
                remaining = deadline - time.monotonic()
                if remaining > 0:
                    time.sleep(remaining)
        except BrokenPipeError:
            feeder_rc = 1
            broken_pipe = True
        finally:
            try:
                process.stdin.close()
            except (BrokenPipeError, OSError):
                pass
        remaining_timeout = max(0.1, args.timeout_seconds - (time.monotonic() - start))
        try:
            renderer_rc = process.wait(timeout=remaining_timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            process.kill()
            renderer_rc = process.wait(timeout=5)
    end = time.monotonic()

    actual_frames = frame_count(args.paced_render) if args.paced_render.exists() else 0
    elapsed = end - start
    realtime_factor = media_seconds / elapsed if elapsed > 0 else 0.0
    log = args.log.read_text(encoding="utf-8", errors="replace") if args.log.exists() else ""
    xrun_markers = re.findall(r"(?i)\b(?:xrun|underrun|overrun)\b", log)
    minimum_elapsed = media_seconds * 0.85
    maximum_elapsed = media_seconds * 1.35 + 1.5
    passed = (
        feeder_rc == 0
        and renderer_rc == 0
        and not broken_pipe
        and not timed_out
        and expected_frames > 0
        and actual_frames == expected_frames
        and not xrun_markers
        and minimum_elapsed <= elapsed <= maximum_elapsed
    )
    payload = {
        "schema_version": 1,
        "status": "pass" if passed else "fail",
        "bursts": bursts,
        "burst_bytes": burst_bytes,
        "expected_frames": expected_frames,
        "actual_frames": actual_frames,
        "frames_per_burst": frames_per_burst,
        "media_seconds": media_seconds,
        "elapsed_seconds": elapsed,
        "realtime_factor": realtime_factor,
        "minimum_elapsed_seconds": minimum_elapsed,
        "maximum_elapsed_seconds": maximum_elapsed,
        "xrun_marker_count": len(xrun_markers),
        "feeder_exit_code": feeder_rc,
        "renderer_exit_code": renderer_rc,
        "broken_pipe": broken_pipe,
        "timed_out": timed_out,
    }
    args.report.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        "AURORA-MOVING-PACED-{} bursts={} frames={} media_seconds={:.3f} "
        "elapsed_seconds={:.3f} realtime_factor={:.3f} xruns={}".format(
            "PASS" if passed else "FAIL",
            bursts,
            actual_frames,
            media_seconds,
            elapsed,
            realtime_factor,
            len(xrun_markers),
        )
    )
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
