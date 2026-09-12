#!/usr/bin/env python3
"""Compare Aurora 3D VBAP against pinned OAR 7.1.4 object semantics."""

from __future__ import annotations

import argparse
import json
import math
import re
from pathlib import Path

CHANNELS = 12
LFE = 3
SPATIAL = tuple(i for i in range(CHANNELS) if i != LFE)
TOP = (8, 9, 10, 11)
BASE = (0, 1, 2, 4, 5, 6, 7)
EXPECTED_ANCHORS = {
    "FL": 0,
    "FR": 1,
    "FC": 2,
    "SL": 4,
    "SR": 5,
    "SBL": 6,
    "SBR": 7,
    "TFL": 8,
    "TFR": 9,
    "TRL": 10,
    "TRR": 11,
}
MIRROR = {0: 1, 1: 0, 2: 2, 3: 3, 4: 5, 5: 4, 6: 7, 7: 6, 8: 9, 9: 8, 10: 11, 11: 10}
MIRROR_PROBES = (("FL", "FR"), ("SL", "SR"), ("SBL", "SBR"), ("TFL", "TFR"), ("TRL", "TRR"))
LINE_RE = re.compile(r"^(?:AURORA714|OAR714)-PROBE\s+(.*)$")


def fail(message: str) -> None:
    raise ValueError(message)


def parse_fields(payload: str) -> dict[str, str]:
    result: dict[str, str] = {}
    for token in payload.split():
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        result[key] = value
    return result


def parse_log(path: Path) -> dict[str, dict[str, object]]:
    probes: dict[str, dict[str, object]] = {}
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        match = LINE_RE.match(raw.strip())
        if not match:
            continue
        fields = parse_fields(match.group(1))
        name = fields.get("name")
        if not name:
            continue
        if name in probes:
            fail(f"{path}: duplicate probe {name}")
        try:
            channels = [float(fields[f"ch{i}"]) for i in range(CHANNELS)]
            probes[name] = {
                "azimuth_degrees": float(fields["azimuth"]),
                "elevation_degrees": float(fields["elevation"]),
                "gain_db": float(fields["gain_db"]),
                "channels": channels,
            }
        except (KeyError, ValueError) as exc:
            fail(f"{path}: malformed probe {name}: {exc}")
    required = set(EXPECTED_ANCHORS) | {"TOPC", "MIDC", "FCM6"}
    missing = sorted(required - probes.keys())
    if missing:
        fail(f"{path}: missing probes {missing}")
    return probes


def spatial_norm(values: list[float]) -> float:
    return math.sqrt(sum(values[i] * values[i] for i in SPATIAL))


def normalized(values: list[float]) -> list[float]:
    norm = spatial_norm(values)
    if not math.isfinite(norm) or norm <= 1e-12:
        fail("non-finite or silent spatial vector")
    return [value / norm if i != LFE else 0.0 for i, value in enumerate(values)]


def fraction(values: list[float], indices: tuple[int, ...]) -> float:
    total = sum(values[i] * values[i] for i in SPATIAL)
    if total <= 1e-24:
        return 0.0
    return sum(values[i] * values[i] for i in indices) / total


def dominant(values: list[float]) -> int:
    return max(SPATIAL, key=lambda i: abs(values[i]))


def max_abs_delta(first: list[float], second: list[float]) -> float:
    return max(abs(a - b) for a, b in zip(first, second))


def mirrored(values: list[float]) -> list[float]:
    return [values[MIRROR[i]] for i in range(CHANNELS)]


def analyze(aurora: dict[str, dict[str, object]], oar: dict[str, dict[str, object]]) -> dict[str, object]:
    failures: list[str] = []
    evidence: dict[str, object] = {}

    for renderer_name, probes in (("aurora", aurora), ("oar", oar)):
        renderer_evidence: dict[str, object] = {}
        for name, expected_index in EXPECTED_ANCHORS.items():
            values = probes[name]["channels"]
            assert isinstance(values, list)
            if any(not math.isfinite(value) for value in values):
                failures.append(f"{renderer_name}:{name}: non-finite output")
                continue
            lfe_abs = abs(values[LFE])
            norm = spatial_norm(values)
            if norm <= 1e-12:
                failures.append(f"{renderer_name}:{name}: silent spatial output")
                continue
            if dominant(values) != expected_index:
                failures.append(
                    f"{renderer_name}:{name}: dominant channel {dominant(values)} != expected {expected_index}"
                )
            if lfe_abs / norm > 1e-5:
                failures.append(f"{renderer_name}:{name}: object energy leaked into LFE")
            renderer_evidence[name] = {
                "dominant_channel": dominant(values),
                "lfe_to_spatial_ratio": lfe_abs / norm,
                "normalized": normalized(values),
            }

        for left_name, right_name in MIRROR_PROBES:
            left = normalized(probes[left_name]["channels"])  # type: ignore[arg-type]
            right = normalized(probes[right_name]["channels"])  # type: ignore[arg-type]
            delta = max_abs_delta(mirrored(left), right)
            renderer_evidence[f"mirror_{left_name}_{right_name}"] = delta
            if delta > 1e-4:
                failures.append(f"{renderer_name}:{left_name}/{right_name}: mirror delta {delta:.6g}")

        topc_values = probes["TOPC"]["channels"]
        midc_values = probes["MIDC"]["channels"]
        assert isinstance(topc_values, list) and isinstance(midc_values, list)
        topc_fraction = fraction(topc_values, TOP)
        midc_fraction = fraction(midc_values, TOP)
        renderer_evidence["top_center_top_energy_fraction"] = topc_fraction
        renderer_evidence["mid_center_top_energy_fraction"] = midc_fraction
        if topc_fraction < 0.99:
            failures.append(f"{renderer_name}:TOPC: top energy fraction {topc_fraction:.6f} < 0.99")
        if not 0.05 < midc_fraction < 0.95:
            failures.append(
                f"{renderer_name}:MIDC: expected mixed base/top energy, got top fraction {midc_fraction:.6f}"
            )
        if abs(topc_values[LFE]) > 1e-8 or abs(midc_values[LFE]) > 1e-8:
            failures.append(f"{renderer_name}: height probes leaked into LFE")

        fc = probes["FC"]["channels"]
        fcm6 = probes["FCM6"]["channels"]
        assert isinstance(fc, list) and isinstance(fcm6, list)
        gain_ratio = spatial_norm(fcm6) / spatial_norm(fc)
        renderer_evidence["minus_6db_ratio"] = gain_ratio
        if abs(gain_ratio - 10 ** (-6.0 / 20.0)) > 1e-3:
            failures.append(f"{renderer_name}: -6 dB ratio mismatch {gain_ratio:.9f}")

        evidence[renderer_name] = renderer_evidence

    cross_anchor_deltas: dict[str, float] = {}
    for name in EXPECTED_ANCHORS:
        a = normalized(aurora[name]["channels"])  # type: ignore[arg-type]
        o = normalized(oar[name]["channels"])  # type: ignore[arg-type]
        delta = max_abs_delta(a, o)
        cross_anchor_deltas[name] = delta
        if delta > 0.05:
            failures.append(f"cross:{name}: normalized anchor delta {delta:.6f} > 0.05")

    a_topc = normalized(aurora["TOPC"]["channels"])  # type: ignore[arg-type]
    o_topc = normalized(oar["TOPC"]["channels"])  # type: ignore[arg-type]
    topc_delta = max_abs_delta(a_topc, o_topc)
    if topc_delta > 0.10:
        failures.append(f"cross:TOPC normalized delta {topc_delta:.6f} > 0.10")

    a_gain = evidence["aurora"]["minus_6db_ratio"]  # type: ignore[index]
    o_gain = evidence["oar"]["minus_6db_ratio"]  # type: ignore[index]
    gain_delta = abs(float(a_gain) - float(o_gain))
    if gain_delta > 1e-3:
        failures.append(f"cross: -6 dB ratio delta {gain_delta:.9f} > 0.001")

    evidence["cross"] = {
        "anchor_normalized_max_abs_delta": cross_anchor_deltas,
        "top_center_normalized_max_abs_delta": topc_delta,
        "mid_center_top_energy_fraction_delta": abs(
            float(evidence["aurora"]["mid_center_top_energy_fraction"])  # type: ignore[index]
            - float(evidence["oar"]["mid_center_top_energy_fraction"])  # type: ignore[index]
        ),
        "minus_6db_ratio_delta": gain_delta,
    }

    return {
        "schema_version": 1,
        "scope": "7.1.4 object anchor, mirror, height-layer, LFE-isolation and gain semantics",
        "verdict": "pass" if not failures else "fail",
        "failures": failures,
        "evidence": evidence,
        "truth_boundary": (
            "This differential validates selected 7.1.4 object-rendering invariants against pinned OAR. "
            "It does not prove IAMF bitstream decoding, arbitrary-position algorithm equivalence, binaural/head-tracking equivalence, "
            "physical playback, room acoustics, proprietary renderer equivalence, or certification. MIDC is diagnostic across "
            "different 3D/layerwise algorithms except for the requirement that both base and height layers participate."
        ),
    }


def self_test() -> None:
    base = [0.0] * CHANNELS
    base[2] = 1.0
    assert dominant(base) == 2
    assert math.isclose(fraction(base, TOP), 0.0)
    top = [0.0] * CHANNELS
    top[8] = 2 ** -0.5
    top[9] = 2 ** -0.5
    assert math.isclose(fraction(top, TOP), 1.0)
    left = [0.0] * CHANNELS
    left[0] = 1.0
    right = [0.0] * CHANNELS
    right[1] = 1.0
    assert max_abs_delta(mirrored(left), right) == 0.0
    print("oar-aurora-714-differential: SELF-TEST PASS")


def main() -> None:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("self-test")
    analyze_parser = sub.add_parser("analyze")
    analyze_parser.add_argument("--aurora-log", type=Path, required=True)
    analyze_parser.add_argument("--oar-log", type=Path, required=True)
    analyze_parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    if args.command == "self-test":
        self_test()
        return

    aurora = parse_log(args.aurora_log)
    oar = parse_log(args.oar_log)
    report = analyze(aurora, oar)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"verdict": report["verdict"], "failures": report["failures"]}, indent=2))
    if report["verdict"] != "pass":
        raise SystemExit(1)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"oar-aurora-714-differential: ERROR: {exc}")
        raise SystemExit(2)
