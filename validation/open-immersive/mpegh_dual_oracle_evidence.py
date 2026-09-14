#!/usr/bin/env python3
"""Bounded cross-vendor evidence for Aurora's external MPEG-H decoder oracles."""

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


def read_f32le(path: Path) -> array:
    raw = path.read_bytes()
    if not raw or len(raw) % 4:
        raise ValueError(f"{path}: empty or misaligned f32le PCM")
    values = array("f")
    values.frombytes(raw)
    if sys.byteorder != "little":
        values.byteswap()
    if any(not math.isfinite(value) for value in values):
        raise ValueError(f"{path}: non-finite PCM")
    return values


def first_active_frame(values: array, channels: int, threshold: float) -> int:
    for sample_index, value in enumerate(values):
        if abs(value) > threshold:
            return sample_index // channels
    raise ValueError("PCM is silent")


def channel_values(values: array, channels: int, channel: int, start_frame: int) -> list[float]:
    return list(values[start_frame * channels + channel :: channels])


def correlation(a: list[float], b: list[float], lag: int, limit: int | None = None) -> tuple[float, int]:
    if lag >= 0:
        a_start, b_start = lag, 0
    else:
        a_start, b_start = 0, -lag
    count = min(len(a) - a_start, len(b) - b_start)
    if limit is not None:
        count = min(count, limit)
    if count <= 1:
        raise ValueError("not enough aligned samples")

    sum_a = sum_b = sum_aa = sum_bb = sum_ab = 0.0
    for index in range(count):
        x = a[a_start + index]
        y = b[b_start + index]
        sum_a += x
        sum_b += y
        sum_aa += x * x
        sum_bb += y * y
        sum_ab += x * y

    inv = 1.0 / count
    cov = sum_ab - sum_a * sum_b * inv
    var_a = sum_aa - sum_a * sum_a * inv
    var_b = sum_bb - sum_b * sum_b * inv
    if var_a <= 0.0 or var_b <= 0.0:
        raise ValueError("zero-variance active channel")
    return cov / math.sqrt(var_a * var_b), count


def rms(values: list[float], start: int, count: int) -> float:
    if count <= 0:
        return 0.0
    return math.sqrt(sum(value * value for value in values[start : start + count]) / count)


def best_channel_alignment(
    a: list[float], b: list[float], lag_search: int, probe_frames: int = 48_000
) -> tuple[int, float]:
    best_lag = 0
    best_corr = -2.0
    for lag in range(-lag_search, lag_search + 1):
        try:
            corr, _ = correlation(a, b, lag, probe_frames)
        except ValueError:
            continue
        if corr > best_corr:
            best_lag = lag
            best_corr = corr
    if best_corr < -1.0:
        raise ValueError("unable to align active channel")
    return best_lag, best_corr


def analyze(
    pcm_a_path: Path,
    pcm_b_path: Path,
    config_path: Path,
    sample_rate_a: int,
    sample_rate_b: int,
    channels_a: int,
    channels_b: int,
    fixture_path: Path | None,
) -> dict[str, object]:
    config = json.loads(config_path.read_text(encoding="utf-8"))
    policy = config["comparison"]

    if channels_a <= 0 or channels_b <= 0:
        raise ValueError("invalid channel count")
    if policy["require_equal_channel_count"] and channels_a != channels_b:
        raise ValueError(f"channel mismatch: {channels_a} != {channels_b}")
    if policy["require_equal_sample_rate"] and sample_rate_a != sample_rate_b:
        raise ValueError(f"sample-rate mismatch: {sample_rate_a} != {sample_rate_b}")
    expected_channels = int(config["fixture"]["expected_channels"])
    if channels_a != expected_channels or channels_b != expected_channels:
        raise ValueError(
            f"expected CICP target to render {expected_channels} channels, got {channels_a}/{channels_b}"
        )

    a = read_f32le(pcm_a_path)
    b = read_f32le(pcm_b_path)
    if len(a) % channels_a or len(b) % channels_b:
        raise ValueError("PCM sample count is not frame aligned")

    frames_a = len(a) // channels_a
    frames_b = len(b) // channels_b
    threshold = float(policy["leading_activity_threshold"])
    start_a = first_active_frame(a, channels_a, threshold)
    start_b = first_active_frame(b, channels_b, threshold)
    active_frames_a = frames_a - start_a
    active_frames_b = frames_b - start_b
    frame_delta = abs(active_frames_a - active_frames_b)
    if frame_delta > int(policy["maximum_post_trim_frame_count_delta"]):
        raise ValueError(f"post-trim frame-count delta {frame_delta} exceeds policy")

    minimum_common = int(policy["minimum_common_frames"])
    if min(active_frames_a, active_frames_b) < minimum_common:
        raise ValueError("not enough common rendered frames")

    lag_search = int(policy["residual_lag_search_frames"])
    min_corr = float(policy["minimum_active_channel_correlation"])
    min_ratio = float(policy["minimum_rms_ratio"])
    max_ratio = float(policy["maximum_rms_ratio"])
    inactive_rms = threshold

    channel_reports: list[dict[str, object]] = []
    for channel in range(expected_channels):
        chan_a = channel_values(a, channels_a, channel, start_a)
        chan_b = channel_values(b, channels_b, channel, start_b)
        base_count = min(len(chan_a), len(chan_b))
        base_rms_a = rms(chan_a, 0, base_count)
        base_rms_b = rms(chan_b, 0, base_count)
        active_a = base_rms_a > inactive_rms
        active_b = base_rms_b > inactive_rms
        if active_a != active_b:
            raise ValueError(f"channel {channel}: activity mismatch")
        if not active_a:
            channel_reports.append(
                {
                    "channel": channel,
                    "active": False,
                    "rms_a": base_rms_a,
                    "rms_b": base_rms_b,
                }
            )
            continue

        lag, probe_corr = best_channel_alignment(chan_a, chan_b, lag_search)
        if lag >= 0:
            a_start, b_start = lag, 0
        else:
            a_start, b_start = 0, -lag
        count = min(len(chan_a) - a_start, len(chan_b) - b_start)
        corr, _ = correlation(chan_a, chan_b, lag, None)
        rms_a = rms(chan_a, a_start, count)
        rms_b = rms(chan_b, b_start, count)
        if rms_a <= 0.0 or rms_b <= 0.0:
            raise ValueError(f"channel {channel}: unexpected zero RMS")
        ratio = rms_b / rms_a
        if corr < min_corr:
            raise ValueError(f"channel {channel}: correlation {corr:.6f} < {min_corr:.6f}")
        if not (min_ratio <= ratio <= max_ratio):
            raise ValueError(
                f"channel {channel}: RMS ratio {ratio:.6f} outside [{min_ratio}, {max_ratio}]"
            )
        channel_reports.append(
            {
                "channel": channel,
                "active": True,
                "residual_lag_frames": lag,
                "probe_correlation": probe_corr,
                "full_correlation": corr,
                "rms_a": rms_a,
                "rms_b": rms_b,
                "rms_ratio_b_over_a": ratio,
                "compared_frames": count,
            }
        )

    if not any(bool(report["active"]) for report in channel_reports):
        raise ValueError("all channels are silent")

    fixture_sha256 = None
    if fixture_path is not None:
        fixture_sha256 = hashlib.sha256(fixture_path.read_bytes()).hexdigest()

    return {
        "schema_version": 1,
        "verdict": "pass",
        "reference_a": "fraunhofer_mpeghdec",
        "reference_b": "ittiam_libmpegh",
        "sample_rate": sample_rate_a,
        "channels": channels_a,
        "frames_a": frames_a,
        "frames_b": frames_b,
        "first_active_frame_a": start_a,
        "first_active_frame_b": start_b,
        "post_trim_frame_count_delta": frame_delta,
        "fixture_sha256": fixture_sha256,
        "pcm_a_sha256": hashlib.sha256(pcm_a_path.read_bytes()).hexdigest(),
        "pcm_b_sha256": hashlib.sha256(pcm_b_path.read_bytes()).hexdigest(),
        "channel_metrics": channel_reports,
        "pins": {
            "fraunhofer_mpeghdec": config["oracles"]["fraunhofer_mpeghdec"]["commit"],
            "ittiam_libmpegh": config["oracles"]["ittiam_libmpegh"]["commit"],
        },
        "truth_boundary": config["truth_boundary"],
    }


def write_f32(path: Path, samples: list[float]) -> None:
    path.write_bytes(b"".join(struct.pack("<f", value) for value in samples))


def self_test() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        config = root / "config.json"
        a_path = root / "a.f32"
        b_path = root / "b.f32"
        fixture = root / "fixture.bin"
        fixture.write_bytes(b"fixture")
        config.write_text(
            json.dumps(
                {
                    "oracles": {
                        "fraunhofer_mpeghdec": {"commit": "a" * 40},
                        "ittiam_libmpegh": {"commit": "b" * 40},
                    },
                    "fixture": {"expected_channels": 2},
                    "comparison": {
                        "minimum_common_frames": 1000,
                        "maximum_post_trim_frame_count_delta": 16,
                        "leading_activity_threshold": 1e-7,
                        "residual_lag_search_frames": 8,
                        "minimum_active_channel_correlation": 0.98,
                        "minimum_rms_ratio": 0.5,
                        "maximum_rms_ratio": 2.0,
                        "require_equal_sample_rate": True,
                        "require_equal_channel_count": True,
                    },
                    "truth_boundary": "self-test",
                }
            ),
            encoding="utf-8",
        )
        frames = 6000
        a_samples: list[float] = []
        for n in range(frames):
            sample = 0.25 * math.sin(2.0 * math.pi * 1000.0 * n / 48000.0)
            a_samples.extend((sample, sample * 0.5))
        b_samples = [0.0] * 6 + [value * 0.9 for value in a_samples]
        write_f32(a_path, a_samples)
        write_f32(b_path, b_samples)
        report = analyze(a_path, b_path, config, 48000, 48000, 2, 2, fixture)
        assert report["verdict"] == "pass"

        inverted = b_samples.copy()
        for index in range(0, len(inverted), 2):
            inverted[index] = -inverted[index]
        write_f32(b_path, inverted)
        try:
            analyze(a_path, b_path, config, 48000, 48000, 2, 2, fixture)
        except ValueError:
            pass
        else:
            raise AssertionError("phase-inverted active channel must fail")
    print("MPEGH-DUAL-ORACLE-EVIDENCE-SELF-TEST-PASS")


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "self-test":
        self_test()
        return 0

    parser = argparse.ArgumentParser()
    parser.add_argument("pcm_a", type=Path)
    parser.add_argument("pcm_b", type=Path)
    parser.add_argument("config", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--sample-rate-a", type=int, required=True)
    parser.add_argument("--sample-rate-b", type=int, required=True)
    parser.add_argument("--channels-a", type=int, required=True)
    parser.add_argument("--channels-b", type=int, required=True)
    parser.add_argument("--fixture", type=Path)
    args = parser.parse_args()

    try:
        report = analyze(
            args.pcm_a,
            args.pcm_b,
            args.config,
            args.sample_rate_a,
            args.sample_rate_b,
            args.channels_a,
            args.channels_b,
            args.fixture,
        )
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(f"MPEGH-DUAL-ORACLE-EVIDENCE-FAIL: {error}", file=sys.stderr)
        return 1

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    active = [entry for entry in report["channel_metrics"] if entry["active"]]
    minimum_corr = min(float(entry["full_correlation"]) for entry in active)
    print(
        "MPEGH-DUAL-ORACLE-EVIDENCE-PASS "
        f"channels={report['channels']} frames={min(report['frames_a'], report['frames_b'])} "
        f"min_corr={minimum_corr:.6f} frame_delta={report['post_trim_frame_count_delta']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
