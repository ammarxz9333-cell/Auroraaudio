#!/usr/bin/env python3
"""Deterministic validation-only channel-upmix matrix.

This is a measurement/reference model, not Aurora's selected production upmixer.
It intentionally does not decode or reconstruct JOC/OAMD object metadata.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from typing import Callable

SOURCE_51 = ["FL", "FR", "FC", "LFE", "SL", "SR"]
SOURCE_71 = ["FL", "FR", "FC", "LFE", "SBL", "SBR", "SL", "SR"]
TARGET_714 = ["FL", "FR", "FC", "LFE", "SBL", "SBR", "SL", "SR", "TFL", "TFR", "TRL", "TRR"]
TARGET_1114 = [
    "FL",
    "FR",
    "FC",
    "FWL",
    "FWR",
    "SL",
    "SR",
    "RWL",
    "RWR",
    "SBL",
    "SBR",
    "LFE",
    "TFL",
    "TFR",
    "TRL",
    "TRR",
]

FREQUENCIES = {
    "FL": 220.0,
    "FR": 300.0,
    "FC": 380.0,
    "LFE": 60.0,
    "SL": 460.0,
    "SR": 540.0,
    "SBL": 620.0,
    "SBR": 700.0,
}


def tone(frequency: float, amplitude: float, frames: int, sample_rate: int) -> list[float]:
    angular = 2.0 * math.pi * frequency / sample_rate
    return [amplitude * math.sin(angular * index) for index in range(frames)]


def zeros(frames: int) -> list[float]:
    return [0.0] * frames


def generate_source(layout: list[str], amplitude: float, frames: int, sample_rate: int) -> dict[str, list[float]]:
    return {
        channel: tone(FREQUENCIES[channel], amplitude, frames, sample_rate)
        for channel in layout
    }


def isolated_source(layout: list[str], active: str, amplitude: float, frames: int, sample_rate: int) -> dict[str, list[float]]:
    result = {channel: zeros(frames) for channel in layout}
    result[active] = tone(FREQUENCIES[active], amplitude, frames, sample_rate)
    return result


def delayed_mix(frames: int, delay: int, terms: list[tuple[float, list[float]]]) -> list[float]:
    output = [0.0] * frames
    for index in range(delay, frames):
        source_index = index - delay
        output[index] = sum(coefficient * signal[source_index] for coefficient, signal in terms)
    return output


def synthetic_heights(source: dict[str, list[float]], frames: int) -> dict[str, list[float]]:
    fl = source["FL"]
    fr = source["FR"]
    sl = source["SL"]
    sr = source["SR"]
    return {
        "TFL": delayed_mix(frames, 11, [(0.22, fl), (-0.22, fr)]),
        "TFR": delayed_mix(frames, 17, [(0.22, fr), (-0.22, fl)]),
        "TRL": delayed_mix(frames, 23, [(0.22, sl), (-0.22, sr)]),
        "TRR": delayed_mix(frames, 29, [(0.22, sr), (-0.22, sl)]),
    }


def derived_backs_from_51(source: dict[str, list[float]], frames: int) -> dict[str, list[float]]:
    return {
        "SBL": delayed_mix(frames, 13, [(0.45, source["SL"])]),
        "SBR": delayed_mix(frames, 17, [(0.45, source["SR"])]),
    }


def synthetic_wides(source: dict[str, list[float]], backs: dict[str, list[float]], frames: int) -> dict[str, list[float]]:
    return {
        "FWL": delayed_mix(frames, 7, [(0.22, source["FL"]), (0.14, source["SL"])]),
        "FWR": delayed_mix(frames, 9, [(0.22, source["FR"]), (0.14, source["SR"])]),
        "RWL": delayed_mix(frames, 13, [(0.18, source["SL"]), (0.25, backs["SBL"])]),
        "RWR": delayed_mix(frames, 15, [(0.18, source["SR"]), (0.25, backs["SBR"])]),
    }


def upmix_51_to_714(source: dict[str, list[float]], frames: int) -> tuple[dict[str, list[float]], dict[str, list[str]]]:
    backs = derived_backs_from_51(source, frames)
    heights = synthetic_heights(source, frames)
    output = {
        "FL": source["FL"].copy(),
        "FR": source["FR"].copy(),
        "FC": source["FC"].copy(),
        "LFE": source["LFE"].copy(),
        "SBL": backs["SBL"],
        "SBR": backs["SBR"],
        "SL": source["SL"].copy(),
        "SR": source["SR"].copy(),
        **heights,
    }
    groups = {
        "preserved": SOURCE_51,
        "derived_bed": ["SBL", "SBR"],
        "wides": [],
        "heights": ["TFL", "TFR", "TRL", "TRR"],
    }
    return output, groups


def upmix_51_to_1114(source: dict[str, list[float]], frames: int) -> tuple[dict[str, list[float]], dict[str, list[str]]]:
    backs = derived_backs_from_51(source, frames)
    wides = synthetic_wides(source, backs, frames)
    heights = synthetic_heights(source, frames)
    output = {
        "FL": source["FL"].copy(),
        "FR": source["FR"].copy(),
        "FC": source["FC"].copy(),
        **wides,
        "SL": source["SL"].copy(),
        "SR": source["SR"].copy(),
        "SBL": backs["SBL"],
        "SBR": backs["SBR"],
        "LFE": source["LFE"].copy(),
        **heights,
    }
    groups = {
        "preserved": SOURCE_51,
        "derived_bed": ["SBL", "SBR"],
        "wides": ["FWL", "FWR", "RWL", "RWR"],
        "heights": ["TFL", "TFR", "TRL", "TRR"],
    }
    return output, groups


def upmix_71_to_1114(source: dict[str, list[float]], frames: int) -> tuple[dict[str, list[float]], dict[str, list[str]]]:
    backs = {"SBL": source["SBL"], "SBR": source["SBR"]}
    wides = synthetic_wides(source, backs, frames)
    heights = synthetic_heights(source, frames)
    output = {
        "FL": source["FL"].copy(),
        "FR": source["FR"].copy(),
        "FC": source["FC"].copy(),
        **wides,
        "SL": source["SL"].copy(),
        "SR": source["SR"].copy(),
        "SBL": source["SBL"].copy(),
        "SBR": source["SBR"].copy(),
        "LFE": source["LFE"].copy(),
        **heights,
    }
    groups = {
        "preserved": SOURCE_71,
        "derived_bed": [],
        "wides": ["FWL", "FWR", "RWL", "RWR"],
        "heights": ["TFL", "TFR", "TRL", "TRR"],
    }
    return output, groups


def mean(values: list[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def energy(values: list[float]) -> float:
    return sum(value * value for value in values)


def rms(values: list[float]) -> float:
    return math.sqrt(energy(values) / len(values)) if values else 0.0


def peak(channels: dict[str, list[float]]) -> float:
    return max((abs(value) for signal in channels.values() for value in signal), default=0.0)


def max_abs_difference(left: list[float], right: list[float]) -> float:
    return max((abs(a - b) for a, b in zip(left, right)), default=0.0)


def correlation(left: list[float], right: list[float], skip: int) -> float:
    a = left[skip:]
    b = right[skip:]
    if not a or len(a) != len(b):
        return 0.0
    mean_a = mean(a)
    mean_b = mean(b)
    numerator = sum((x - mean_a) * (y - mean_b) for x, y in zip(a, b))
    denom_a = sum((x - mean_a) ** 2 for x in a)
    denom_b = sum((y - mean_b) ** 2 for y in b)
    denominator = math.sqrt(denom_a * denom_b)
    return numerator / denominator if denominator > 1.0e-20 else 0.0


def steady_rms_modulation(signal: list[float], sample_rate: int) -> float:
    block = sample_rate // 20  # 50 ms; test tones are integer-cycle multiples of 20 Hz.
    start = sample_rate // 10  # ignore delay/filter priming region.
    values = [rms(signal[index:index + block]) for index in range(start, len(signal) - block + 1, block)]
    active = [value for value in values if value > 1.0e-12]
    if len(active) < 2:
        return 0.0
    average = mean(active)
    return (max(active) - min(active)) / average if average > 0.0 else 0.0


def synthetic_energy_fraction(output: dict[str, list[float]], channels: list[str], source: dict[str, list[float]]) -> float:
    source_energy = sum(energy(signal) for signal in source.values())
    synthetic_energy = sum(energy(output[channel]) for channel in channels)
    return synthetic_energy / source_energy if source_energy > 0.0 else 0.0


def max_synthetic_source_correlation(
    output: dict[str, list[float]],
    synthetic_channels: list[str],
    source: dict[str, list[float]],
    sample_rate: int,
) -> float:
    skip = sample_rate // 10
    values = [
        abs(correlation(output[channel], source_signal, skip))
        for channel in synthetic_channels
        for source_signal in source.values()
    ]
    return max(values, default=0.0)


def max_synthetic_dc(output: dict[str, list[float]], channels: list[str], sample_rate: int) -> float:
    skip = sample_rate // 10
    return max((abs(mean(output[channel][skip:])) for channel in channels), default=0.0)


def max_synthetic_rms_modulation(output: dict[str, list[float]], channels: list[str], sample_rate: int) -> float:
    return max((steady_rms_modulation(output[channel], sample_rate) for channel in channels), default=0.0)


def source_target_map(lane_id: str) -> dict[str, str]:
    if lane_id == "5.1-to-7.1.4":
        return {channel: channel for channel in SOURCE_51}
    if lane_id == "5.1-to-11.1.4-custom":
        return {channel: channel for channel in SOURCE_51}
    if lane_id == "7.1-to-11.1.4-custom":
        return {channel: channel for channel in SOURCE_71}
    raise ValueError(lane_id)


def evaluate_lane(
    lane_id: str,
    source_layout: list[str],
    target_layout: list[str],
    upmixer: Callable[[dict[str, list[float]], int], tuple[dict[str, list[float]], dict[str, list[str]]]],
    config: dict,
) -> dict:
    sample_rate = int(config["sample_rate"])
    frames = round(float(config["duration_seconds"]) * sample_rate)
    amplitude = float(config["source_peak"])
    gates = config["gates"]
    source = generate_source(source_layout, amplitude, frames, sample_rate)
    output, groups = upmixer(source, frames)
    repeat, repeat_groups = upmixer(source, frames)
    assert groups == repeat_groups
    assert list(output) == list(repeat)
    assert set(output) == set(target_layout), (lane_id, sorted(output), sorted(target_layout))

    mapping = source_target_map(lane_id)
    bed_error = max(
        max_abs_difference(source[source_channel], output[target_channel])
        for source_channel, target_channel in mapping.items()
    )

    synthetic = groups["derived_bed"] + groups["wides"] + groups["heights"]
    deterministic = all(output[channel] == repeat[channel] for channel in output)
    finite = all(math.isfinite(value) for signal in output.values() for value in signal)

    center_only = isolated_source(source_layout, "FC", amplitude, frames, sample_rate)
    center_output, center_groups = upmixer(center_only, frames)
    center_synthetic = center_groups["derived_bed"] + center_groups["wides"] + center_groups["heights"]
    center_leakage = max((peak({channel: center_output[channel]}) for channel in center_synthetic), default=0.0)

    lfe_only = isolated_source(source_layout, "LFE", amplitude, frames, sample_rate)
    lfe_output, lfe_groups = upmixer(lfe_only, frames)
    lfe_synthetic = lfe_groups["derived_bed"] + lfe_groups["wides"] + lfe_groups["heights"]
    lfe_leakage = max((peak({channel: lfe_output[channel]}) for channel in lfe_synthetic), default=0.0)
    lfe_preservation_error = max_abs_difference(lfe_only["LFE"], lfe_output["LFE"])

    height_rms_values = [rms(output[channel][sample_rate // 10:]) for channel in groups["heights"]]
    height_energy_fraction = synthetic_energy_fraction(output, groups["heights"], source)
    wide_energy_fraction = synthetic_energy_fraction(output, groups["wides"], source)
    synthetic_correlation = max_synthetic_source_correlation(output, synthetic, source, sample_rate)
    synthetic_dc = max_synthetic_dc(output, synthetic, sample_rate)
    rms_modulation = max_synthetic_rms_modulation(output, synthetic, sample_rate)
    output_peak = peak(output)

    metrics = {
        "bed_preservation_max_abs_error": bed_error,
        "center_synthetic_leakage_peak": center_leakage,
        "lfe_synthetic_leakage_peak": lfe_leakage,
        "lfe_preservation_max_abs_error": lfe_preservation_error,
        "height_rms_min": min(height_rms_values),
        "height_rms_max": max(height_rms_values),
        "height_energy_fraction": height_energy_fraction,
        "wide_energy_fraction": wide_energy_fraction,
        "output_peak": output_peak,
        "synthetic_dc_max_abs": synthetic_dc,
        "synthetic_zero_lag_source_correlation_max_abs": synthetic_correlation,
        "steady_block_rms_modulation_fraction_max": rms_modulation,
        "source_channels": len(source_layout),
        "target_channels": len(target_layout),
        "synthetic_channels": len(synthetic),
    }
    checks = {
        "deterministic": deterministic,
        "finite": finite,
        "bed_preserved": bed_error <= gates["max_bed_preservation_error"],
        "center_anchored": center_leakage <= gates["max_center_synthetic_leakage"],
        "lfe_isolated": lfe_leakage <= gates["max_lfe_synthetic_leakage"],
        "lfe_preserved": lfe_preservation_error <= gates["max_bed_preservation_error"],
        "height_active": min(height_rms_values) >= gates["min_height_rms"],
        "height_energy_bounded": height_energy_fraction <= gates["max_height_energy_fraction"],
        "wide_energy_bounded": wide_energy_fraction <= gates["max_wide_energy_fraction"],
        "headroom": output_peak <= gates["max_output_peak"],
        "dc_stable": synthetic_dc <= gates["max_synthetic_dc"],
        "decorrelated": synthetic_correlation <= gates["max_zero_lag_source_correlation"],
        "steady_state_stable": rms_modulation <= gates["max_steady_block_rms_modulation_fraction"],
    }
    return {
        "lane": lane_id,
        "source_layout": source_layout,
        "target_layout": target_layout,
        "groups": groups,
        "metrics": metrics,
        "checks": checks,
        "passed": all(checks.values()),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, default=Path("config/upmix-validation-v1.json"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    config = json.loads(args.config.read_text(encoding="utf-8"))
    lane_specs = {
        "5.1-to-7.1.4": (SOURCE_51, TARGET_714, upmix_51_to_714),
        "5.1-to-11.1.4-custom": (SOURCE_51, TARGET_1114, upmix_51_to_1114),
        "7.1-to-11.1.4-custom": (SOURCE_71, TARGET_1114, upmix_71_to_1114),
    }
    results = []
    configured_ids = [lane["id"] for lane in config["lanes"]]
    if configured_ids != list(lane_specs):
        raise SystemExit(f"unexpected lane order/configuration: {configured_ids}")
    for lane in config["lanes"]:
        source_layout, target_layout, upmixer = lane_specs[lane["id"]]
        result = evaluate_lane(lane["id"], source_layout, target_layout, upmixer, config)
        result["declared_status"] = lane["status"]
        results.append(result)
        print(
            f"{lane['id']}: passed={result['passed']} "
            f"bed_error={result['metrics']['bed_preservation_max_abs_error']:.3g} "
            f"height_energy={result['metrics']['height_energy_fraction']:.4f} "
            f"wide_energy={result['metrics']['wide_energy_fraction']:.4f} "
            f"peak={result['metrics']['output_peak']:.4f}"
        )

    evidence = {
        "schema_version": 1,
        "artifact": "channel-upmix-validation-matrix",
        "classification": config["classification"],
        "sample_rate": config["sample_rate"],
        "duration_seconds": config["duration_seconds"],
        "gates": config["gates"],
        "lanes": results,
        "passed": all(result["passed"] for result in results),
        "truth_boundary": config["truth_boundary"],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if not evidence["passed"]:
        failed = [result["lane"] for result in results if not result["passed"]]
        raise SystemExit(f"upmix validation failed: {failed}")
    print("UPMIX-MATRIX-PASS")


if __name__ == "__main__":
    main()
