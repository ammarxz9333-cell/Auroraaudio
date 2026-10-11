#!/usr/bin/env python3
"""Same-host reproducible codec/channel-mapping throughput microbenchmark.

Measures *software batch throughput* only. Never use this as a proxy for live
eARC latency, Dolby Atmos object quality, or physical multichannel output.
One build must win measured accuracy/corpus BEFORE throughput decides a tie.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import shutil
import statistics
import subprocess
import time

from aurora_streaming_bed import PAN_51_SIDE
from test_streaming_bed import raw_pcm


def run_ffmpeg(binary: str, command: list[str], payload: bytes,
               *, timeout: float = 90.0) -> subprocess.CompletedProcess:
    return subprocess.run(
        [binary, "-hide_banner", "-loglevel", "error", "-nostdin", *command],
        input=payload, capture_output=True, check=False, timeout=timeout,
    )


def make_fixture(binary: str, seconds: int) -> bytes:
    # Deterministic 5.1(side) multitone fixture, same compressed input reused
    # for every version/candidate, with fixed encoder for both comparisons.
    samples = raw_pcm() * (seconds * 5 // 2)
    encoded = run_ffmpeg(binary, [
        "-threads", "1", "-f", "f32le", "-ar", "48000", "-ac", "6",
        "-channel_layout", "5.1(side)", "-i", "pipe:0",
        "-c:a", "eac3", "-b:a", "640k", "-f", "spdif", "pipe:1",
    ], samples)
    if encoded.returncode or len(encoded.stdout) < 4096:
        raise RuntimeError("reference DD+ fixture encoding failed")
    return encoded.stdout


def bench(binary: str, payload: bytes, seconds: int, runs: int) -> dict:
    # Actual DD+ decoder plus exact Aurora fixed 12ch channel mapping.
    command = [
        "-threads", "1", "-f", "spdif", "-i", "pipe:0",
        "-af", PAN_51_SIDE, "-ar", "48000",
        "-c:a", "pcm_s32le", "-f", "null", "-",
    ]
    observations = []
    for _ in range(runs + 1):
        begin = time.perf_counter()
        result = run_ffmpeg(binary, command, payload)
        elapsed = time.perf_counter() - begin
        if result.returncode != 0:
            raise RuntimeError(
                f"decoder failed for {binary}: {result.stderr[:500].decode(errors='replace')}"
            )
        observations.append(elapsed)
    median = statistics.median(observations[1:])
    return {
        "binary": str(Path(shutil.which(binary) or binary).resolve()),
        "fixture_duration_seconds": seconds,
        "measured_repetitions": runs,
        "median_batch_elapsed_seconds": round(median, 6),
        "batch_decode_throughput_realtime_factor": round(seconds / median, 2),
        "pessimistic_batch_elapsed_seconds": round(max(observations[1:]), 6),
        "warmup_excluded": True,
    }


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--candidate-bin", required=True)
    p.add_argument("--baseline-bin")
    p.add_argument("--fixture-seconds", type=int, default=12)
    p.add_argument("--runs", type=int, default=3)
    args = p.parse_args()
    if not 2 <= args.fixture_seconds <= 120 or not 2 <= args.runs <= 15:
        raise ValueError("unsafe microbenchmark duration/runs")
    for binary in filter(None, [args.candidate_bin, args.baseline_bin]):
        if not shutil.which(binary) and not Path(binary).is_file():
            raise RuntimeError(f"FFmpeg binary missing: {binary}")
    payload = make_fixture(args.candidate_bin, args.fixture_seconds)
    result = {
        "schema_version": 1,
        "metric": "offline-single-thread-eac3-5.1-to-12-channel-null-throughput",
        "signal": "fixed synthesized multitone 5.1(side) IEC61937, no actual streaming service",
        "candidate": bench(args.candidate_bin, payload, args.fixture_seconds, args.runs),
        "baseline": None,
        "faster_winner": None,
        "native_immersive_object_decoding_proven": False,
        "actual_live_playback_latency_measured": False,
        "physical_audio_verified": False,
    }
    if args.baseline_bin:
        result["baseline"] = bench(
            args.baseline_bin, payload, args.fixture_seconds, args.runs
        )
        candidate_t = result["candidate"]["median_batch_elapsed_seconds"]
        baseline_t = result["baseline"]["median_batch_elapsed_seconds"]
        # Treat marginal differences as inconclusive, not a performance winner.
        if candidate_t < baseline_t * 0.90:
            result["faster_winner"] = "candidate"
        elif baseline_t < candidate_t * 0.90:
            result["faster_winner"] = "baseline"
        else:
            result["faster_winner"] = "INCONCLUSIVE-within-10pct"
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
