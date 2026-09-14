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
from typing import Any


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


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def first_active_frame(values: array, channels: int, threshold: float) -> int:
    for sample_index, value in enumerate(values):
        if abs(value) > threshold:
            return sample_index // channels
    raise ValueError("PCM is silent")


def first_active_sample(values: list[float], threshold: float) -> int | None:
    for index, value in enumerate(values):
        if abs(value) > threshold:
            return index
    return None


def channel_values(values: array, channels: int, channel: int, start_frame: int = 0) -> list[float]:
    return list(values[start_frame * channels + channel :: channels])


def moments(values: list[float], start: int = 0, count: int | None = None) -> dict[str, float | int]:
    if count is None:
        count = len(values) - start
    count = max(0, min(count, len(values) - start))
    if count == 0:
        return {"count": 0, "mean": 0.0, "rms": 0.0, "variance": 0.0, "peak": 0.0}

    total = total_sq = 0.0
    peak = 0.0
    for value in values[start : start + count]:
        total += value
        total_sq += value * value
        peak = max(peak, abs(value))
    mean = total / count
    mean_sq = total_sq / count
    variance = max(0.0, mean_sq - mean * mean)
    return {
        "count": count,
        "mean": mean,
        "rms": math.sqrt(max(0.0, mean_sq)),
        "variance": variance,
        "peak": peak,
    }


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
        raise ValueError("zero-variance signal")
    return cov / math.sqrt(var_a * var_b), count


def best_alignment(
    a: list[float], b: list[float], lag_search: int, probe_frames: int = 48_000
) -> dict[str, Any]:
    best: dict[str, Any] | None = None
    reasons: set[str] = set()
    for lag in range(-lag_search, lag_search + 1):
        try:
            corr, count = correlation(a, b, lag, probe_frames)
        except ValueError as error:
            reasons.add(str(error))
            continue
        candidate = {
            "available": True,
            "lag_frames": lag,
            "probe_correlation": corr,
            "probe_frames": count,
        }
        if best is None or corr > float(best["probe_correlation"]):
            best = candidate
    if best is not None:
        return best
    return {
        "available": False,
        "reason": ", ".join(sorted(reasons)) if reasons else "no valid lag candidate",
    }


def aligned_metrics(a: list[float], b: list[float], lag: int) -> dict[str, Any]:
    if lag >= 0:
        a_start, b_start = lag, 0
    else:
        a_start, b_start = 0, -lag
    count = min(len(a) - a_start, len(b) - b_start)
    corr, _ = correlation(a, b, lag, None)
    stats_a = moments(a, a_start, count)
    stats_b = moments(b, b_start, count)
    rms_a = float(stats_a["rms"])
    rms_b = float(stats_b["rms"])
    return {
        "full_correlation": corr,
        "compared_frames": count,
        "rms_a": rms_a,
        "rms_b": rms_b,
        "rms_ratio_b_over_a": (rms_b / rms_a) if rms_a > 0.0 else None,
    }


def cross_channel_diagnostics(
    channels_a: list[list[float]],
    channels_b: list[list[float]],
    active_a: list[bool],
    active_b: list[bool],
    lag_search: int,
) -> list[dict[str, Any]]:
    diagnostics: list[dict[str, Any]] = []
    for index_a, chan_a in enumerate(channels_a):
        if not active_a[index_a]:
            diagnostics.append({"channel_a": index_a, "active": False, "best_matches": []})
            continue
        matches: list[dict[str, Any]] = []
        unavailable: list[dict[str, Any]] = []
        for index_b, chan_b in enumerate(channels_b):
            if not active_b[index_b]:
                continue
            alignment = best_alignment(chan_a, chan_b, lag_search)
            if not alignment["available"]:
                unavailable.append({"channel_b": index_b, "reason": alignment["reason"]})
                continue
            matches.append(
                {
                    "channel_b": index_b,
                    "lag_frames": alignment["lag_frames"],
                    "probe_correlation": alignment["probe_correlation"],
                }
            )
        matches.sort(key=lambda item: float(item["probe_correlation"]), reverse=True)
        diagnostics.append(
            {
                "channel_a": index_a,
                "active": True,
                "best_matches": matches[:3],
                "unavailable_matches": unavailable,
            }
        )
    return diagnostics


def analyze(
    pcm_a_path: Path,
    pcm_b_path: Path,
    config_path: Path,
    sample_rate_a: int,
    sample_rate_b: int,
    channels_a: int,
    channels_b: int,
    fixture_path: Path | None,
) -> dict[str, Any]:
    config = json.loads(config_path.read_text(encoding="utf-8"))
    policy = config["comparison"]
    failures: list[str] = []

    if channels_a <= 0 or channels_b <= 0:
        raise ValueError("invalid channel count")
    expected_channels = int(config["fixture"]["expected_channels"])
    if bool(policy["require_equal_channel_count"]) and channels_a != channels_b:
        failures.append(f"channel mismatch: {channels_a} != {channels_b}")
    if bool(policy["require_equal_sample_rate"]) and sample_rate_a != sample_rate_b:
        failures.append(f"sample-rate mismatch: {sample_rate_a} != {sample_rate_b}")
    if channels_a != expected_channels or channels_b != expected_channels:
        failures.append(
            f"expected CICP target to render {expected_channels} channels, got {channels_a}/{channels_b}"
        )

    a = read_f32le(pcm_a_path)
    b = read_f32le(pcm_b_path)
    if len(a) % channels_a or len(b) % channels_b:
        raise ValueError("PCM sample count is not frame aligned")

    frames_a = len(a) // channels_a
    frames_b = len(b) // channels_b
    threshold = float(policy["leading_activity_threshold"])
    global_start_a = first_active_frame(a, channels_a, threshold)
    global_start_b = first_active_frame(b, channels_b, threshold)
    active_frames_a = frames_a - global_start_a
    active_frames_b = frames_b - global_start_b
    frame_delta = abs(active_frames_a - active_frames_b)
    if frame_delta > int(policy["maximum_post_trim_frame_count_delta"]):
        failures.append(f"post-trim frame-count delta {frame_delta} exceeds policy")

    minimum_common = int(policy["minimum_common_frames"])
    if min(active_frames_a, active_frames_b) < minimum_common:
        failures.append("not enough common rendered frames")

    lag_search = int(policy["residual_lag_search_frames"])
    min_corr = float(policy["minimum_active_channel_correlation"])
    min_ratio = float(policy["minimum_rms_ratio"])
    max_ratio = float(policy["maximum_rms_ratio"])
    global_onset_offset = global_start_a - global_start_b

    compare_channels = min(channels_a, channels_b, expected_channels)
    full_channels_a = [channel_values(a, channels_a, channel) for channel in range(compare_channels)]
    full_channels_b = [channel_values(b, channels_b, channel) for channel in range(compare_channels)]
    channel_starts_a = [first_active_sample(values, threshold) for values in full_channels_a]
    channel_starts_b = [first_active_sample(values, threshold) for values in full_channels_b]
    active_a = [start is not None for start in channel_starts_a]
    active_b = [start is not None for start in channel_starts_b]

    trimmed_channels_a: list[list[float]] = []
    trimmed_channels_b: list[list[float]] = []
    for channel in range(compare_channels):
        start_a = channel_starts_a[channel]
        start_b = channel_starts_b[channel]
        trimmed_channels_a.append(full_channels_a[channel][start_a:] if start_a is not None else [])
        trimmed_channels_b.append(full_channels_b[channel][start_b:] if start_b is not None else [])

    channel_reports: list[dict[str, Any]] = []
    for channel in range(compare_channels):
        start_a = channel_starts_a[channel]
        start_b = channel_starts_b[channel]
        base: dict[str, Any] = {
            "channel": channel,
            "active_a": active_a[channel],
            "active_b": active_b[channel],
            "first_active_frame_a": start_a,
            "first_active_frame_b": start_b,
            "full_stats_a": moments(full_channels_a[channel]),
            "full_stats_b": moments(full_channels_b[channel]),
            "active_stats_a": moments(trimmed_channels_a[channel]),
            "active_stats_b": moments(trimmed_channels_b[channel]),
        }
        if active_a[channel] != active_b[channel]:
            reason = f"channel {channel}: activity mismatch"
            failures.append(reason)
            base["same_index_alignment"] = {"available": False, "reason": reason}
            channel_reports.append(base)
            continue
        if not active_a[channel]:
            base["same_index_alignment"] = {"available": False, "reason": "both channels inactive"}
            channel_reports.append(base)
            continue

        assert start_a is not None and start_b is not None
        onset_offset = start_a - start_b
        relative_onset_error = onset_offset - global_onset_offset
        base["onset_offset_a_minus_b"] = onset_offset
        base["global_onset_offset_a_minus_b"] = global_onset_offset
        base["relative_onset_error_frames"] = relative_onset_error
        if abs(relative_onset_error) > lag_search:
            failures.append(
                f"channel {channel}: relative onset error {relative_onset_error} frames "
                f"exceeds +/-{lag_search}"
            )

        if min(len(trimmed_channels_a[channel]), len(trimmed_channels_b[channel])) < minimum_common:
            failures.append(f"channel {channel}: not enough common active frames")
            base["same_index_alignment"] = {
                "available": False,
                "reason": "not enough common active frames",
            }
            channel_reports.append(base)
            continue

        alignment = best_alignment(trimmed_channels_a[channel], trimmed_channels_b[channel], lag_search)
        base["same_index_alignment"] = alignment
        if not alignment["available"]:
            reason = f"channel {channel}: same-index alignment unavailable ({alignment['reason']})"
            failures.append(reason)
            channel_reports.append(base)
            continue

        try:
            metrics = aligned_metrics(
                trimmed_channels_a[channel],
                trimmed_channels_b[channel],
                int(alignment["lag_frames"]),
            )
        except ValueError as error:
            reason = f"channel {channel}: full aligned metrics unavailable ({error})"
            failures.append(reason)
            base["same_index_metrics_error"] = str(error)
            channel_reports.append(base)
            continue

        base["same_index_metrics"] = metrics
        corr = float(metrics["full_correlation"])
        ratio_value = metrics["rms_ratio_b_over_a"]
        if corr < min_corr:
            failures.append(f"channel {channel}: correlation {corr:.6f} < {min_corr:.6f}")
        if ratio_value is None:
            failures.append(f"channel {channel}: zero RMS prevents ratio")
        else:
            ratio = float(ratio_value)
            if not (min_ratio <= ratio <= max_ratio):
                failures.append(
                    f"channel {channel}: RMS ratio {ratio:.6f} outside [{min_ratio}, {max_ratio}]"
                )
        channel_reports.append(base)

    if not any(active_a) or not any(active_b):
        failures.append("all compared channels are silent in at least one reference")

    cross_diagnostics = cross_channel_diagnostics(
        trimmed_channels_a, trimmed_channels_b, active_a, active_b, lag_search
    )

    fixture_sha256 = sha256(fixture_path) if fixture_path is not None else None
    report: dict[str, Any] = {
        "schema_version": 3,
        "verdict": "fail" if failures else "pass",
        "failure_reasons": failures,
        "reference_a": "fraunhofer_mpeghdec",
        "reference_b": "ittiam_libmpegh",
        "sample_rate_a": sample_rate_a,
        "sample_rate_b": sample_rate_b,
        "channels_a": channels_a,
        "channels_b": channels_b,
        "frames_a": frames_a,
        "frames_b": frames_b,
        "first_active_frame_a": global_start_a,
        "first_active_frame_b": global_start_b,
        "global_onset_offset_a_minus_b": global_onset_offset,
        "active_frames_a": active_frames_a,
        "active_frames_b": active_frames_b,
        "post_trim_frame_count_delta": frame_delta,
        "fixture_sha256": fixture_sha256,
        "pcm_a_sha256": sha256(pcm_a_path),
        "pcm_b_sha256": sha256(pcm_b_path),
        "channel_metrics": channel_reports,
        "cross_channel_diagnostics": cross_diagnostics,
        "policy": {
            "minimum_common_frames": minimum_common,
            "maximum_post_trim_frame_count_delta": int(policy["maximum_post_trim_frame_count_delta"]),
            "leading_activity_threshold": threshold,
            "residual_lag_search_frames": lag_search,
            "minimum_active_channel_correlation": min_corr,
            "minimum_rms_ratio": min_ratio,
            "maximum_rms_ratio": max_ratio,
            "per_channel_onset_consistency_rule": (
                "abs((channel_start_a-channel_start_b)-"
                "(global_start_a-global_start_b)) <= residual_lag_search_frames"
            ),
        },
        "pins": {
            "fraunhofer_mpeghdec": config["oracles"]["fraunhofer_mpeghdec"]["commit"],
            "ittiam_libmpegh": config["oracles"]["ittiam_libmpegh"]["commit"],
        },
        "truth_boundary": config["truth_boundary"],
    }
    return report


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
                        "require_equal_channel_count": True
                    },
                    "truth_boundary": "self-test"
                }
            ),
            encoding="utf-8",
        )
        frames = 6000
        a_samples: list[float] = []
        b_samples: list[float] = []
        for n in range(frames):
            left = 0.25 * math.sin(2.0 * math.pi * 1000.0 * n / 48000.0)
            delayed_right = (
                0.0
                if n < 1200
                else 0.10 * math.sin(2.0 * math.pi * 2000.0 * (n - 1200) / 48000.0)
            )
            a_samples.extend((left, delayed_right))
            b_samples.extend((left * 0.9, delayed_right * 0.9))
        b_samples = [0.0] * 6 + b_samples
        write_f32(a_path, a_samples)
        write_f32(b_path, b_samples)
        report = analyze(a_path, b_path, config, 48000, 48000, 2, 2, fixture)
        assert report["verdict"] == "pass"
        assert report["channel_metrics"][1]["first_active_frame_a"] >= 1200
        assert report["channel_metrics"][1]["same_index_alignment"]["available"]

        inverted = b_samples.copy()
        for index in range(0, len(inverted), 2):
            inverted[index] = -inverted[index]
        write_f32(b_path, inverted)
        report = analyze(a_path, b_path, config, 48000, 48000, 2, 2, fixture)
        assert report["verdict"] == "fail"
        assert any("channel 0: correlation" in reason for reason in report["failure_reasons"])

        swapped: list[float] = []
        for n in range(frames):
            left = 0.25 * math.sin(2.0 * math.pi * 1000.0 * n / 48000.0)
            right = 0.10 * math.sin(2.0 * math.pi * 2000.0 * n / 48000.0)
            swapped.extend((right, left))
        write_f32(b_path, swapped)
        report = analyze(a_path, b_path, config, 48000, 48000, 2, 2, fixture)
        assert report["verdict"] == "fail"
        best = report["cross_channel_diagnostics"][0]["best_matches"][0]
        assert best["channel_b"] == 1
        assert float(best["probe_correlation"]) > 0.99
    print("MPEGH-DUAL-ORACLE-EVIDENCE-SELF-TEST-PASS")


def failure_report(error: Exception, config_path: Path | None = None) -> dict[str, Any]:
    report: dict[str, Any] = {
        "schema_version": 3,
        "verdict": "fail",
        "failure_reasons": [f"fatal analysis error: {error}"],
    }
    if config_path is not None:
        try:
            config = json.loads(config_path.read_text(encoding="utf-8"))
            report["truth_boundary"] = config.get("truth_boundary")
            report["pins"] = {
                "fraunhofer_mpeghdec": config.get("oracles", {})
                .get("fraunhofer_mpeghdec", {})
                .get("commit"),
                "ittiam_libmpegh": config.get("oracles", {}).get("ittiam_libmpegh", {}).get("commit"),
            }
        except Exception:
            pass
    return report


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
        report = failure_report(error, args.config)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if report["verdict"] != "pass":
        print("MPEGH-DUAL-ORACLE-EVIDENCE-FAIL", file=sys.stderr)
        for reason in report.get("failure_reasons", []):
            print(f"- {reason}", file=sys.stderr)
        for channel in report.get("channel_metrics", []):
            active_stats_a = channel["active_stats_a"]
            active_stats_b = channel["active_stats_b"]
            print(
                "diagnostic "
                f"channel={channel['channel']} active_a={channel['active_a']} active_b={channel['active_b']} "
                f"start_a={channel['first_active_frame_a']} start_b={channel['first_active_frame_b']} "
                f"rms_a={float(active_stats_a['rms']):.9g} rms_b={float(active_stats_b['rms']):.9g} "
                f"var_a={float(active_stats_a['variance']):.9g} var_b={float(active_stats_b['variance']):.9g}",
                file=sys.stderr,
            )
        return 1

    active_metrics = [
        entry["same_index_metrics"]
        for entry in report["channel_metrics"]
        if "same_index_metrics" in entry
    ]
    minimum_corr = min(float(entry["full_correlation"]) for entry in active_metrics)
    print(
        "MPEGH-DUAL-ORACLE-EVIDENCE-PASS "
        f"channels={report['channels_a']} frames={min(report['frames_a'], report['frames_b'])} "
        f"min_corr={minimum_corr:.6f} frame_delta={report['post_trim_frame_count_delta']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
