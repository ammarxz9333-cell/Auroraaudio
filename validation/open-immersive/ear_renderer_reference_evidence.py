#!/usr/bin/env python3
"""Bounded evidence analyzer for the exact-pin EBU ADM Renderer lane.

This script intentionally validates only the public upstream fixture and layouts
named in config/ear-renderer-reference-v1.json. It is not a general ADM
conformance checker and it does not provide an Aurora runtime renderer.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import pathlib
import struct
import sys
from typing import Iterable, Sequence


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_f32le(path: pathlib.Path, channels: int) -> tuple[list[float], int]:
    raw = path.read_bytes()
    if len(raw) % 4:
        raise ValueError(f"{path}: byte length is not a multiple of float32")
    count = len(raw) // 4
    if count % channels:
        raise ValueError(
            f"{path}: {count} samples are not divisible by {channels} channels"
        )
    values = list(struct.unpack(f"<{count}f", raw))
    if not all(math.isfinite(value) for value in values):
        raise ValueError(f"{path}: PCM contains NaN or Inf")
    return values, count // channels


def rms(values: Iterable[float]) -> float:
    values = list(values)
    if not values:
        return 0.0
    return math.sqrt(sum(value * value for value in values) / len(values))


def channel_rms(values: Sequence[float], channels: int) -> list[float]:
    frames = len(values) // channels
    out: list[float] = []
    for channel in range(channels):
        energy = 0.0
        for frame in range(frames):
            value = values[frame * channels + channel]
            energy += value * value
        out.append(math.sqrt(energy / frames) if frames else 0.0)
    return out


def expected_four_five_zero(
    input_pcm: Sequence[float], frames: int, sample_rate: int
) -> list[float]:
    """Reproduce the mapping asserted by upstream test_integrate.py::test_render."""
    if len(input_pcm) != frames * 4:
        raise ValueError("4+5+0 expectation requires four-channel input")

    output = [0.0] * (frames * 10)
    switch_frame = sample_rate // 8
    for frame in range(frames):
        in_base = frame * 4
        out_base = frame * 10
        ch0 = input_pcm[in_base]
        ch1 = input_pcm[in_base + 1]
        ch2 = input_pcm[in_base + 2]
        ch3 = input_pcm[in_base + 3]

        if frame < switch_frame:
            output[out_base + 2] += ch0  # M+000
        else:
            output[out_base + 1] += ch0  # M-030 after the zero-length jump

        output[out_base + 6] += ch1  # U+030
        output[out_base + 7] += ch2  # U-030 direct speaker
        output[out_base + 0] += ch2  # M+030 direct speaker
        output[out_base + 3] += ch3  # LFE1
    return output


def max_abs_error(actual: Sequence[float], expected: Sequence[float]) -> float:
    if len(actual) != len(expected):
        return math.inf
    return max((abs(a - b) for a, b in zip(actual, expected)), default=0.0)


def self_test() -> int:
    sample_rate = 64
    frames = 16
    input_pcm: list[float] = []
    for frame in range(frames):
        input_pcm.extend(
            [
                0.01 * (frame + 1),
                -0.02 * (frame + 1),
                0.03 * (frame + 1),
                -0.04 * (frame + 1),
            ]
        )

    expected = expected_four_five_zero(input_pcm, frames, sample_rate)
    if max_abs_error(expected, expected) != 0.0:
        raise AssertionError("identity comparison failed")

    corrupted = list(expected)
    corrupted[7] += 0.1
    if max_abs_error(corrupted, expected) <= 0.05:
        raise AssertionError("semantic corruption was not detected")

    rms_values = channel_rms(expected, 10)
    if sum(value > 1e-9 for value in rms_values) < 5:
        raise AssertionError("synthetic fixture did not activate expected channels")

    print("EAR-REFERENCE-ANALYZER-SELF-TEST-PASS")
    return 0


def analyze(args: argparse.Namespace) -> int:
    config_path = pathlib.Path(args.config)
    config = json.loads(config_path.read_text(encoding="utf-8"))

    reference = config["reference"]
    fixture_cfg = config["fixture"]
    layout_45 = config["layouts"]["4+5+0"]
    layout_47 = config["layouts"]["4+7+0"]
    acceptance = config["acceptance"]

    input_path = pathlib.Path(args.input_f32)
    render_45_path = pathlib.Path(args.render_4_5_f32)
    render_47_path = pathlib.Path(args.render_4_7_f32)
    fixture_path = pathlib.Path(args.fixture)
    metadata_path = pathlib.Path(args.metadata)
    generated_bwf_path = pathlib.Path(args.generated_bwf)
    upstream_bwf_path = pathlib.Path(args.upstream_bwf)
    summary_path = pathlib.Path(args.summary)

    input_channels = int(fixture_cfg["input_channels"])
    channels_45 = int(layout_45["expected_channels"])
    channels_47 = int(layout_47["expected_channels"])
    expected_rate = int(fixture_cfg["sample_rate"])
    expected_frames = int(fixture_cfg["frames"])

    input_pcm, input_frames = read_f32le(input_path, input_channels)
    render_45, frames_45 = read_f32le(render_45_path, channels_45)
    render_47, frames_47 = read_f32le(render_47_path, channels_47)

    expected_45 = expected_four_five_zero(input_pcm, input_frames, expected_rate)
    semantic_error_45 = max_abs_error(render_45, expected_45)

    rms_45 = channel_rms(render_45, channels_45)
    rms_47 = channel_rms(render_47, channels_47)
    active_47 = sum(value > 1e-8 for value in rms_47)

    generated_bwf_sha = sha256_file(generated_bwf_path)
    upstream_bwf_sha = sha256_file(upstream_bwf_path)

    checks = {
        "reference_commit_matches_config": reference["commit"]
        == "5bb17b4278e8e67f24f90efb7a81a8aea7aed4f3",
        "input_rate": int(args.input_rate) == expected_rate,
        "render_4_5_rate": int(args.render_4_5_rate) == expected_rate,
        "render_4_7_rate": int(args.render_4_7_rate) == expected_rate,
        "input_frame_count": input_frames == expected_frames,
        "render_4_5_frame_count": abs(frames_45 - expected_frames)
        <= int(acceptance["rendered_frame_delta_max"]),
        "render_4_7_frame_count": abs(frames_47 - expected_frames)
        <= int(acceptance["rendered_frame_delta_max"]),
        "generated_bwf_matches_upstream": generated_bwf_sha == upstream_bwf_sha,
        "four_five_zero_semantics": semantic_error_45
        <= float(layout_45["max_abs_error"]),
        "four_five_zero_non_silent": rms(render_45) > 1e-8,
        "four_seven_zero_non_silent": rms(render_47) > 1e-8,
        "four_seven_zero_active_channels": active_47
        >= int(layout_47["minimum_active_channels"]),
        "plain_wav_without_adm_failed": int(args.plain_wav_exit) != 0,
    }

    verdict = "PASS" if all(checks.values()) else "FAIL"
    summary = {
        "schema_version": 1,
        "verdict": verdict,
        "reference": {
            "id": reference["id"],
            "version": reference["version"],
            "commit": reference["commit"],
            "integration": reference["integration"],
        },
        "fixture": {
            "input_wav_sha256": sha256_file(fixture_path),
            "metadata_sha256": sha256_file(metadata_path),
            "generated_bwf_sha256": generated_bwf_sha,
            "upstream_bwf_sha256": upstream_bwf_sha,
            "normalized_input_f32_sha256": sha256_file(input_path),
            "input_frames": input_frames,
            "input_channels": input_channels,
            "sample_rate": expected_rate,
        },
        "renders": {
            "4+5+0": {
                "channels": channels_45,
                "frames": frames_45,
                "normalized_f32_sha256": sha256_file(render_45_path),
                "channel_rms": rms_45,
                "max_abs_error_vs_upstream_semantic_expectation": semantic_error_45,
            },
            "4+7+0": {
                "channels": channels_47,
                "frames": frames_47,
                "normalized_f32_sha256": sha256_file(render_47_path),
                "channel_rms": rms_47,
                "active_channels": active_47,
            },
        },
        "negative_probe": {
            "plain_wav_without_adm_exit_code": int(args.plain_wav_exit)
        },
        "checks": checks,
        "truth_boundary": config["truth_boundary"],
    }

    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    print(json.dumps(summary, indent=2, sort_keys=True))
    if verdict != "PASS":
        failed = [name for name, passed in checks.items() if not passed]
        print(f"EAR-RENDERER-REFERENCE-FAIL failed_checks={failed}", file=sys.stderr)
        return 1

    print("EAR-RENDERER-REFERENCE-PASS")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command")
    subparsers.add_parser("self-test")

    analyze_parser = subparsers.add_parser("analyze")
    analyze_parser.add_argument("input_f32")
    analyze_parser.add_argument("render_4_5_f32")
    analyze_parser.add_argument("render_4_7_f32")
    analyze_parser.add_argument("config")
    analyze_parser.add_argument("summary")
    analyze_parser.add_argument("--fixture", required=True)
    analyze_parser.add_argument("--metadata", required=True)
    analyze_parser.add_argument("--generated-bwf", required=True)
    analyze_parser.add_argument("--upstream-bwf", required=True)
    analyze_parser.add_argument("--input-rate", required=True, type=int)
    analyze_parser.add_argument("--render-4-5-rate", required=True, type=int)
    analyze_parser.add_argument("--render-4-7-rate", required=True, type=int)
    analyze_parser.add_argument("--plain-wav-exit", required=True, type=int)
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    if args.command == "self-test":
        return self_test()
    if args.command == "analyze":
        return analyze(args)
    parser.print_help()
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
