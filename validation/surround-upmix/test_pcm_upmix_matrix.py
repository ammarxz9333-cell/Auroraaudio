#!/usr/bin/env python3
"""Software-only decoded-PCM channel-upmix matrix validation."""

import array
import json
import math
import os
from pathlib import Path
import subprocess
import sys

FS = 48_000
SCRIPT = Path(
    sys.argv[1]
    if len(sys.argv) > 1
    else Path(__file__).with_name("aurora-pcm-upmix.sh")
).resolve()
CONFIG_PATH = Path(
    sys.argv[2]
    if len(sys.argv) > 2
    else Path(__file__).parents[2] / "config" / "upmix-validation-v1.json"
).resolve()
CONFIG = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
ACCEPT = CONFIG["acceptance"]
FREQUENCIES = [431, 557, 337, 53, 673, 787, 887, 997]


def run(args, data):
    return subprocess.run(
        args,
        input=data,
        capture_output=True,
        check=True,
        timeout=20,
    )


def as_bytes(values):
    values = array.array("f", values)
    if sys.byteorder != "little":
        values.byteswap()
    return values.tobytes()


def parse_f32(data, channels):
    assert data and len(data) % (channels * 4) == 0, "empty/truncated float PCM"
    values = array.array("f")
    values.frombytes(data)
    if sys.byteorder != "little":
        values.byteswap()
    assert all(math.isfinite(value) for value in values), "non-finite PCM"
    return values


def make_signal(channels, amplitude=0.18, seconds=1.0, mono_pairs=False, stress=False):
    frames = round(FS * seconds)
    interleaved = []
    for frame in range(frames):
        t = frame / FS
        for channel_index in range(channels):
            if stress:
                frequency = 50 if channel_index == 3 else 440
                phase = 0.0 if channel_index % 2 == 0 else math.pi
                gain = 0.78
            elif mono_pairs:
                if channel_index in (0, 1):
                    frequency = 440
                elif channels == 6 and channel_index in (4, 5):
                    frequency = 660
                elif channels == 8 and channel_index in (4, 5):
                    frequency = 770
                elif channels == 8 and channel_index in (6, 7):
                    frequency = 660
                else:
                    frequency = FREQUENCIES[channel_index]
                phase = 0.0
                gain = amplitude
            else:
                frequency = FREQUENCIES[channel_index]
                phase = 0.13 * channel_index
                gain = amplitude
            interleaved.append(gain * math.sin(2.0 * math.pi * frequency * t + phase))
    return parse_f32(as_bytes(interleaved), channels)


def channel(values, channels, channel_index):
    return [values[index] for index in range(channel_index, len(values), channels)]


def rms(values):
    return math.sqrt(sum(value * value for value in values) / len(values))


def peak(values):
    return max(abs(value) for value in values)


def correlation(left, right):
    left_mean = sum(left) / len(left)
    right_mean = sum(right) / len(right)
    left_zero = [value - left_mean for value in left]
    right_zero = [value - right_mean for value in right]
    left_norm = math.sqrt(sum(value * value for value in left_zero))
    right_norm = math.sqrt(sum(value * value for value in right_zero))
    if left_norm < 1e-12 or right_norm < 1e-12:
        return 0.0
    return sum(a * b for a, b in zip(left_zero, right_zero)) / (left_norm * right_norm)


def tone_amplitude(values, frequency):
    omega = 2.0 * math.pi * frequency / FS
    cosine = sum(value * math.cos(omega * index) for index, value in enumerate(values))
    sine = sum(value * math.sin(omega * index) for index, value in enumerate(values))
    return 2.0 * math.sqrt(cosine * cosine + sine * sine) / len(values)


def render(lane, source):
    target_cli = lane["cli_target"]
    process = run(
        ["sh", str(SCRIPT), lane["source_layout"], target_cli],
        source.tobytes(),
    )
    marker = f"source={lane['source_layout']} target={target_cli}".encode()
    assert marker in process.stderr
    assert b"objects_decoded=false" in process.stderr
    return parse_f32(process.stdout, lane["target_channels"])


def isolated_source(channels, channel_index, frequency):
    signal = array.array("f", [0.0] * (FS * channels))
    for frame in range(FS):
        signal[frame * channels + channel_index] = 0.18 * math.sin(
            2.0 * math.pi * frequency * frame / FS
        )
    return signal


def measure_lane(lane):
    source_channels = lane["source_channels"]
    target_channels = lane["target_channels"]
    synthetic = lane["synthetic_channels"]
    bed_map = lane["preserved_bed_map"]

    source = make_signal(source_channels)
    output = render(lane, source)
    assert len(output) // target_channels == FS

    bed_error = max(
        max(
            abs(src - dst)
            for src, dst in zip(
                channel(source, source_channels, source_index),
                channel(output, target_channels, target_index),
            )
        )
        for source_index, target_index in bed_map
    )

    bed_rms = max(rms(channel(source, source_channels, source_index)) for source_index, _ in bed_map)
    synthetic_rms = [rms(channel(output, target_channels, target_index)) for target_index in synthetic]

    correlations = []
    for target_index in synthetic:
        derived = channel(output, target_channels, target_index)
        if rms(derived) > 1e-12:
            correlations.append(
                max(
                    abs(correlation(derived, channel(source, source_channels, source_index)))
                    for source_index, _ in bed_map
                )
            )

    spectral_balances = []
    for probe in lane["spectral_probes"]:
        values = channel(output, target_channels, probe["channel"])
        amplitudes = [tone_amplitude(values, frequency) for frequency in probe["frequencies_hz"]]
        spectral_balances.append(min(amplitudes) / max(amplitudes))

    center_only = render(lane, isolated_source(source_channels, 2, 440))
    lfe_only = render(lane, isolated_source(source_channels, 3, 50))
    center_leak = max(peak(channel(center_only, target_channels, index)) for index in synthetic)
    lfe_leak = max(peak(channel(lfe_only, target_channels, index)) for index in synthetic)

    dual_mono = render(lane, make_signal(source_channels, mono_pairs=True))
    dual_mono_peak = max(peak(channel(dual_mono, target_channels, index)) for index in synthetic)

    low = render(lane, make_signal(source_channels, amplitude=0.045, seconds=0.25))
    high = render(lane, make_signal(source_channels, amplitude=0.18, seconds=0.25))
    low_rms = max(rms(channel(low, target_channels, index)) for index in synthetic)
    high_rms = max(rms(channel(high, target_channels, index)) for index in synthetic)
    linear_ratio = high_rms / low_rms

    stressed = render(lane, make_signal(source_channels, stress=True))
    stress_peak = peak(stressed)

    metrics = {
        "frames": FS,
        "bed_max_abs_error": bed_error,
        "max_synthetic_to_bed_rms_ratio": max(synthetic_rms) / bed_rms,
        "max_abs_synthetic_source_correlation": max(correlations),
        "spectral_pair_balance_min_ratio": min(spectral_balances),
        "center_to_synthetic_peak": center_leak,
        "lfe_to_synthetic_peak": lfe_leak,
        "dual_mono_synthetic_peak": dual_mono_peak,
        "linear_gain_ratio_4x_input": linear_ratio,
        "stress_peak_abs": stress_peak,
    }

    assert metrics["bed_max_abs_error"] <= ACCEPT["bed_max_abs_error"], metrics
    assert metrics["max_synthetic_to_bed_rms_ratio"] <= ACCEPT["max_synthetic_to_bed_rms_ratio"], metrics
    assert metrics["max_abs_synthetic_source_correlation"] <= ACCEPT["max_abs_synthetic_source_correlation"], metrics
    assert metrics["spectral_pair_balance_min_ratio"] >= ACCEPT["min_spectral_pair_balance_ratio"], metrics
    assert metrics["center_to_synthetic_peak"] <= ACCEPT["max_center_to_synthetic_peak"], metrics
    assert metrics["lfe_to_synthetic_peak"] <= ACCEPT["max_lfe_to_synthetic_peak"], metrics
    assert metrics["dual_mono_synthetic_peak"] <= ACCEPT["max_dual_mono_synthetic_peak"], metrics
    assert ACCEPT["linear_gain_ratio_4x_input_min"] <= metrics["linear_gain_ratio_4x_input"] <= ACCEPT["linear_gain_ratio_4x_input_max"], metrics
    assert metrics["stress_peak_abs"] <= ACCEPT["max_stress_peak_abs"], metrics
    return metrics


def main():
    assert CONFIG["schema_version"] == 1
    assert CONFIG["sample_rate"] == FS
    assert CONFIG["objects_decoded"] is False
    assert [lane["id"] for lane in CONFIG["lanes"]] == [
        "5.1->7.1.4",
        "5.1->11.1.4",
        "7.1->11.1.4",
    ]

    evidence = {
        "schema_version": 1,
        "mode": CONFIG["mode"],
        "objects_decoded": False,
        "sample_rate": FS,
        "lanes": {},
        "truth_boundary": CONFIG["truth_boundary"],
    }
    for lane in CONFIG["lanes"]:
        evidence["lanes"][lane["id"]] = measure_lane(lane)

    output = Path(os.environ.get("AURORA_UPMIX_EVIDENCE", "/tmp/pcm-upmix-evidence.json"))
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(evidence, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
