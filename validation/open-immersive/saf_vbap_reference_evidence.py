#!/usr/bin/env python3
"""Prepare and compare Aurora ↔ SAF 3D VBAP reference evidence."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from typing import Any

LFE_ROLE = "low-frequency-effects"
TOP_PREFIX = "top-"


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def acoustic_listener_center(scene: dict[str, Any]) -> tuple[float, float, float]:
    listener = scene["listener"]
    position = listener["position"]
    return (
        float(position["x"]),
        float(position["y"]),
        float(position["z"]) + float(listener["ear_height"]),
    )


def direction(point: dict[str, Any], center: tuple[float, float, float]) -> tuple[float, float, float]:
    dx = float(point["x"]) - center[0]
    dy = float(point["y"]) - center[1]
    dz = float(point["z"]) - center[2]
    radius = math.sqrt(dx * dx + dy * dy + dz * dz)
    if not math.isfinite(radius) or radius <= 1.0e-9:
        raise SystemExit(f"invalid listener-relative point: {point}")

    # SAF uses the common audio convention 0°=front and positive azimuth=left.
    # Aurora's fixture uses +Y=front and -X=left, hence atan2(-X, +Y).
    azimuth = math.degrees(math.atan2(-dx, dy))
    elevation = math.degrees(math.atan2(dz, math.hypot(dx, dy)))
    unit = (dx / radius, dy / radius, dz / radius)
    return azimuth, elevation, unit


def write_direction_file(path: Path, rows: list[tuple[float, float]]) -> None:
    text = [str(len(rows))]
    text.extend(f"{azimuth:.9f} {elevation:.9f}" for azimuth, elevation in rows)
    path.write_text("\n".join(text) + "\n", encoding="utf-8")


def prepare(args: argparse.Namespace) -> None:
    scene = load_json(args.scene)
    trajectory = load_json(args.aurora_trajectory)
    frames = trajectory.get("payload")
    if not isinstance(frames, list) or not frames:
        raise SystemExit("Aurora gain trajectory has no payload frames")

    center = acoustic_listener_center(scene)
    speakers = scene.get("speakers")
    if not isinstance(speakers, list) or not speakers:
        raise SystemExit("scene has no speakers")

    spatial_indices: list[int] = []
    spatial_ids: list[str] = []
    spatial_roles: list[str] = []
    speaker_dirs: list[tuple[float, float]] = []
    speaker_units: list[tuple[float, float, float]] = []
    lfe_indices: list[int] = []

    for index, speaker in enumerate(speakers):
        role = str(speaker["channel_role"])
        if role == LFE_ROLE:
            lfe_indices.append(index)
            continue
        azimuth, elevation, unit = direction(speaker["position"], center)
        spatial_indices.append(index)
        spatial_ids.append(str(speaker["id"]))
        spatial_roles.append(role)
        speaker_dirs.append((azimuth, elevation))
        speaker_units.append(unit)

    if len(lfe_indices) != 1:
        raise SystemExit(f"expected exactly one LFE speaker, found {len(lfe_indices)}")
    if len(spatial_indices) < 4:
        raise SystemExit("3D VBAP differential requires at least four spatial speakers")

    source_dirs: list[tuple[float, float]] = []
    for frame in frames:
        azimuth, elevation, _ = direction(frame["source"], center)
        source_dirs.append((azimuth, elevation))

    args.speaker_out.parent.mkdir(parents=True, exist_ok=True)
    write_direction_file(args.speaker_out, speaker_dirs)
    write_direction_file(args.source_out, source_dirs)

    mapping = {
        "schema_version": 1,
        "coordinate_convention": "Aurora +Y front/-X left converted to SAF 0deg front/+azimuth left",
        "listener_center": list(center),
        "spatial_indices": spatial_indices,
        "spatial_speaker_ids": spatial_ids,
        "spatial_roles": spatial_roles,
        "speaker_unit_vectors_aurora_xyz": [list(row) for row in speaker_units],
        "lfe_indices": lfe_indices,
        "frame_count": len(frames),
    }
    args.mapping_out.write_text(json.dumps(mapping, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"SAF-PREPARE-PASS speakers={len(spatial_indices)} frames={len(frames)}")


def read_saf_gains(path: Path) -> tuple[int, int, int, list[list[float]]]:
    lines = path.read_text(encoding="utf-8").strip().splitlines()
    if len(lines) < 2:
        raise SystemExit("SAF gain output is empty")
    header = lines[0].split()
    if len(header) != 3:
        raise SystemExit("invalid SAF gain header")
    rows, speakers, triangles = (int(value) for value in header)
    gains: list[list[float]] = []
    for line in lines[1:]:
        values = [float(value) for value in line.split()]
        if len(values) != speakers:
            raise SystemExit("SAF gain row has unexpected channel count")
        gains.append(values)
    if len(gains) != rows:
        raise SystemExit(f"SAF gain row count mismatch: header={rows}, actual={len(gains)}")
    return rows, speakers, triangles, gains


def power(values: list[float]) -> float:
    return sum(value * value for value in values)


def cosine_similarity(a: list[float], b: list[float]) -> float:
    denominator = math.sqrt(power(a) * power(b))
    if denominator <= 1.0e-12:
        return 0.0
    return sum(left * right for left, right in zip(a, b)) / denominator


def dominant(values: list[float]) -> int:
    return max(range(len(values)), key=values.__getitem__)


def energy_centroid(gains: list[float], unit_vectors: list[list[float]]) -> tuple[float, float, float] | None:
    total = power(gains)
    if total <= 1.0e-12:
        return None
    x = y = z = 0.0
    for gain, vector in zip(gains, unit_vectors):
        weight = gain * gain / total
        x += weight * float(vector[0])
        y += weight * float(vector[1])
        z += weight * float(vector[2])
    length = math.sqrt(x * x + y * y + z * z)
    if length <= 1.0e-9:
        return None
    return x / length, y / length, z / length


def angular_distance_degrees(a: tuple[float, float, float] | None, b: tuple[float, float, float] | None) -> float:
    if a is None or b is None:
        return 180.0
    dot = max(-1.0, min(1.0, sum(left * right for left, right in zip(a, b))))
    return math.degrees(math.acos(dot))


def compare(args: argparse.Namespace) -> None:
    trajectory = load_json(args.aurora_trajectory)
    mapping = load_json(args.mapping)
    frames = trajectory.get("payload")
    if not isinstance(frames, list) or not frames:
        raise SystemExit("Aurora gain trajectory has no frames")

    rows, saf_speaker_count, triangle_count, saf_rows = read_saf_gains(args.saf_gains)
    spatial_indices = [int(value) for value in mapping["spatial_indices"]]
    lfe_indices = [int(value) for value in mapping["lfe_indices"]]
    roles = [str(value) for value in mapping["spatial_roles"]]
    units = mapping["speaker_unit_vectors_aurora_xyz"]

    if rows != len(frames) or saf_speaker_count != len(spatial_indices):
        raise SystemExit("Aurora/SAF differential dimensions do not match")

    cosine_values: list[float] = []
    centroid_errors: list[float] = []
    height_energy_deltas: list[float] = []
    dominant_matches = 0
    aurora_power_errors: list[float] = []
    saf_power_errors: list[float] = []
    lfe_nonzero_frames = 0
    nonfinite_values = 0
    negative_values = 0

    top_indices = [index for index, role in enumerate(roles) if role.startswith(TOP_PREFIX)]
    if not top_indices:
        raise SystemExit("7.1.4 mapping has no top speakers")

    for frame, saf in zip(frames, saf_rows):
        aurora_all = [float(value) for value in frame["gains"]]
        if max(spatial_indices + lfe_indices) >= len(aurora_all):
            raise SystemExit("Aurora gain vector is shorter than fixture mapping")
        aurora = [aurora_all[index] for index in spatial_indices]

        for value in aurora + saf:
            if not math.isfinite(value):
                nonfinite_values += 1
            if value < -1.0e-6:
                negative_values += 1
        if any(abs(aurora_all[index]) > 1.0e-7 for index in lfe_indices):
            lfe_nonzero_frames += 1

        aurora_power_errors.append(abs(power(aurora) - 1.0))
        saf_power_errors.append(abs(power(saf) - 1.0))
        cosine_values.append(cosine_similarity(aurora, saf))
        dominant_matches += int(dominant(aurora) == dominant(saf))
        centroid_errors.append(
            angular_distance_degrees(energy_centroid(aurora, units), energy_centroid(saf, units))
        )
        aurora_height = sum(aurora[index] ** 2 for index in top_indices)
        saf_height = sum(saf[index] ** 2 for index in top_indices)
        height_energy_deltas.append(abs(aurora_height - saf_height))

    frame_count = len(frames)
    metrics = {
        "frame_count": frame_count,
        "spatial_speaker_count": len(spatial_indices),
        "saf_triangle_count": triangle_count,
        "nonfinite_values": nonfinite_values,
        "negative_values": negative_values,
        "lfe_nonzero_frames": lfe_nonzero_frames,
        "aurora_max_power_error": max(aurora_power_errors),
        "saf_max_power_error": max(saf_power_errors),
        "cosine_similarity_min": min(cosine_values),
        "cosine_similarity_mean": sum(cosine_values) / frame_count,
        "dominant_speaker_agreement_fraction": dominant_matches / frame_count,
        "centroid_error_deg_mean": sum(centroid_errors) / frame_count,
        "centroid_error_deg_max": max(centroid_errors),
        "height_energy_abs_delta_mean": sum(height_energy_deltas) / frame_count,
        "height_energy_abs_delta_max": max(height_energy_deltas),
    }

    thresholds = {
        "max_power_error": args.max_power_error,
        "min_cosine_similarity": args.min_cosine_similarity,
        "min_mean_cosine_similarity": args.min_mean_cosine_similarity,
        "min_dominant_agreement": args.min_dominant_agreement,
        "max_mean_centroid_error_deg": args.max_mean_centroid_error_deg,
        "max_mean_height_energy_delta": args.max_mean_height_energy_delta,
    }
    checks = {
        "finite": nonfinite_values == 0,
        "nonnegative": negative_values == 0,
        "lfe_excluded": lfe_nonzero_frames == 0,
        "aurora_unit_power": metrics["aurora_max_power_error"] <= args.max_power_error,
        "saf_unit_power": metrics["saf_max_power_error"] <= args.max_power_error,
        "minimum_vector_similarity": metrics["cosine_similarity_min"] >= args.min_cosine_similarity,
        "mean_vector_similarity": metrics["cosine_similarity_mean"] >= args.min_mean_cosine_similarity,
        "dominant_speaker_agreement": metrics["dominant_speaker_agreement_fraction"] >= args.min_dominant_agreement,
        "mean_spatial_centroid_error": metrics["centroid_error_deg_mean"] <= args.max_mean_centroid_error_deg,
        "mean_height_energy_delta": metrics["height_energy_abs_delta_mean"] <= args.max_mean_height_energy_delta,
    }
    passed = all(checks.values())

    evidence = {
        "schema_version": 1,
        "artifact": "aurora-saf-vbap-differential",
        "status": "REFERENCE-DIFFERENTIAL-PASS" if passed else "REFERENCE-DIFFERENTIAL-FAIL",
        "scene": "fixtures/scenes/7_1_4_reference.json",
        "saf_commit": "18fd5aba46e20787b51f28f7197a68506c965c07",
        "coordinate_convention": mapping["coordinate_convention"],
        "spatial_speaker_ids": mapping["spatial_speaker_ids"],
        "lfe_indices": lfe_indices,
        "metrics": metrics,
        "thresholds": thresholds,
        "checks": checks,
        "passed": passed,
        "truth_boundary": "Software-only 7.1.4 trajectory differential against pinned SAF 3D VBAP. Similarity thresholds compare normalized gain semantics; they do not require identical triangulation or PCM. This does not establish 11.1.4, binaural/HOA, physical acoustic, proprietary-renderer, or certification equivalence."
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(evidence, indent=2, sort_keys=True))
    if not passed:
        raise SystemExit("Aurora ↔ SAF VBAP differential failed one or more evidence gates")


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    sub = root.add_subparsers(dest="command", required=True)

    prep = sub.add_parser("prepare")
    prep.add_argument("--scene", type=Path, required=True)
    prep.add_argument("--aurora-trajectory", type=Path, required=True)
    prep.add_argument("--speaker-out", type=Path, required=True)
    prep.add_argument("--source-out", type=Path, required=True)
    prep.add_argument("--mapping-out", type=Path, required=True)
    prep.set_defaults(func=prepare)

    cmp = sub.add_parser("compare")
    cmp.add_argument("--aurora-trajectory", type=Path, required=True)
    cmp.add_argument("--mapping", type=Path, required=True)
    cmp.add_argument("--saf-gains", type=Path, required=True)
    cmp.add_argument("--output", type=Path, required=True)
    cmp.add_argument("--max-power-error", type=float, default=0.02)
    cmp.add_argument("--min-cosine-similarity", type=float, default=0.50)
    cmp.add_argument("--min-mean-cosine-similarity", type=float, default=0.85)
    cmp.add_argument("--min-dominant-agreement", type=float, default=0.50)
    cmp.add_argument("--max-mean-centroid-error-deg", type=float, default=25.0)
    cmp.add_argument("--max-mean-height-energy-delta", type=float, default=0.25)
    cmp.set_defaults(func=compare)
    return root


def main() -> None:
    args = parser().parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
