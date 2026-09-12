#!/usr/bin/env python3
"""Compare pinned OAR and Aurora horizontal object-panning semantics."""

from __future__ import annotations

import argparse
import json
import math
import re
import tempfile
from pathlib import Path

LINE = re.compile(
    r"^(?P<source>AURORA|OAR)-PROBE\s+"
    r"azimuth=(?P<azimuth>-?\d+(?:\.\d+)?)\s+"
    r"gain_db=(?P<gain_db>-?\d+(?:\.\d+)?)\s+"
    r"left=(?P<left>-?\d+(?:\.\d+)?)\s+"
    r"right=(?P<right>-?\d+(?:\.\d+)?)$"
)


def parse(path: Path, source: str) -> dict[tuple[float, float], tuple[float, float]]:
    records: dict[tuple[float, float], tuple[float, float]] = {}
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        match = LINE.match(line.strip())
        if not match or match.group("source") != source:
            continue
        key = (float(match.group("azimuth")), float(match.group("gain_db")))
        values = (float(match.group("left")), float(match.group("right")))
        if key in records:
            raise ValueError(f"duplicate {source} probe record {key}")
        if not all(math.isfinite(value) and value >= 0.0 for value in values):
            raise ValueError(f"non-finite/negative {source} probe record {key}: {values}")
        records[key] = values
    required = {(30.0, 0.0), (0.0, 0.0), (-30.0, 0.0), (0.0, -6.0)}
    missing = required - records.keys()
    if missing:
        raise ValueError(f"missing {source} probe records: {sorted(missing)}")
    return records


def magnitude(pair: tuple[float, float]) -> float:
    return math.hypot(*pair)


def normalized(pair: tuple[float, float]) -> tuple[float, float]:
    total = magnitude(pair)
    if total <= 1.0e-12:
        raise ValueError("silent probe record")
    return pair[0] / total, pair[1] / total


def balance_error(pair: tuple[float, float]) -> float:
    largest = max(pair)
    if largest <= 1.0e-12:
        return math.inf
    return abs(pair[0] - pair[1]) / largest


def dominant(pair: tuple[float, float]) -> str:
    left, right = pair
    largest = max(left, right)
    if largest <= 1.0e-12:
        return "silent"
    if abs(left - right) / largest <= 0.08:
        return "balanced"
    return "left" if left > right else "right"


def analyze(aurora_log: Path, oar_log: Path, output: Path) -> int:
    aurora = parse(aurora_log, "AURORA")
    oar = parse(oar_log, "OAR")
    failures: list[str] = []

    for name, records in (("aurora", aurora), ("oar", oar)):
        if dominant(records[(30.0, 0.0)]) != "left":
            failures.append(f"{name}: +30deg does not resolve left")
        if dominant(records[(-30.0, 0.0)]) != "right":
            failures.append(f"{name}: -30deg does not resolve right")
        if dominant(records[(0.0, 0.0)]) != "balanced":
            failures.append(f"{name}: 0deg is not left/right balanced")
        if balance_error(records[(0.0, 0.0)]) > 0.08:
            failures.append(f"{name}: center balance exceeds 8% tolerance")

        positive = normalized(records[(30.0, 0.0)])
        negative = normalized(records[(-30.0, 0.0)])
        if abs(positive[0] - negative[1]) > 0.08 or abs(positive[1] - negative[0]) > 0.08:
            failures.append(f"{name}: mirrored +/-30deg energy is not symmetric")

        baseline = magnitude(records[(0.0, 0.0)])
        attenuated = magnitude(records[(0.0, -6.0)])
        ratio = attenuated / baseline if baseline > 1.0e-12 else math.inf
        if not 0.45 <= ratio <= 0.56:
            failures.append(f"{name}: -6dB amplitude ratio {ratio:.6f} outside [0.45, 0.56]")

    cross_deltas: dict[str, float] = {}
    for key in ((30.0, 0.0), (0.0, 0.0), (-30.0, 0.0)):
        a = normalized(aurora[key])
        o = normalized(oar[key])
        delta = max(abs(a[0] - o[0]), abs(a[1] - o[1]))
        cross_deltas[f"azimuth_{int(key[0])}"] = delta
        if delta > 0.15:
            failures.append(
                f"cross-render normalized energy delta at {key[0]:.0f}deg is {delta:.6f}, exceeds 0.15"
            )
        if dominant(aurora[key]) != dominant(oar[key]):
            failures.append(f"cross-render dominant-channel class differs at {key[0]:.0f}deg")

    aurora_gain_ratio = magnitude(aurora[(0.0, -6.0)]) / magnitude(aurora[(0.0, 0.0)])
    oar_gain_ratio = magnitude(oar[(0.0, -6.0)]) / magnitude(oar[(0.0, 0.0)])
    if abs(aurora_gain_ratio - oar_gain_ratio) > 0.05:
        failures.append(
            f"cross-render -6dB amplitude-ratio delta {abs(aurora_gain_ratio - oar_gain_ratio):.6f} exceeds 0.05"
        )

    def serial(records: dict[tuple[float, float], tuple[float, float]]) -> list[dict[str, object]]:
        out = []
        for (azimuth, gain_db), pair in sorted(records.items()):
            norm = normalized(pair)
            out.append(
                {
                    "azimuth_degrees": azimuth,
                    "gain_db": gain_db,
                    "left": pair[0],
                    "right": pair[1],
                    "normalized_left": norm[0],
                    "normalized_right": norm[1],
                    "dominant": dominant(pair),
                }
            )
        return out

    report = {
        "schema_version": 1,
        "verdict": "pass" if not failures else "reject",
        "scope": "horizontal stereo object-panning semantics only",
        "aurora": serial(aurora),
        "oar": serial(oar),
        "comparison": {
            "normalized_energy_max_abs_delta": cross_deltas,
            "aurora_minus_6db_ratio": aurora_gain_ratio,
            "oar_minus_6db_ratio": oar_gain_ratio,
            "gain_ratio_delta": abs(aurora_gain_ratio - oar_gain_ratio),
        },
        "failures": failures,
        "truth_boundary": (
            "This differential lane compares horizontal stereo panning/gain invariants only. "
            "It does not prove IAMF bitstream decoding, 3D/height rendering, binaural rendering, "
            "raw PCM equivalence, physical playback, proprietary-renderer equivalence, or certification."
        ),
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        f"AURORA-OAR-OBJECT-DIFFERENTIAL-{report['verdict'].upper()} "
        f"gain_delta={report['comparison']['gain_ratio_delta']:.6f} "
        f"max_energy_delta={max(cross_deltas.values()):.6f}"
    )
    for failure in failures:
        print(f"REJECT: {failure}")
    return 0 if not failures else 1


def self_test() -> int:
    aurora_text = """AURORA-PROBE azimuth=30.000 gain_db=0.000 left=1.000000000 right=0.000000000
AURORA-PROBE azimuth=0.000 gain_db=0.000 left=0.707106780 right=0.707106780
AURORA-PROBE azimuth=-30.000 gain_db=0.000 left=0.000000000 right=1.000000000
AURORA-PROBE azimuth=0.000 gain_db=-6.000 left=0.354393000 right=0.354393000
"""
    oar_text = """OAR-PROBE azimuth=30.000 gain_db=0.000 left=0.176000000 right=0.000000000
OAR-PROBE azimuth=0.000 gain_db=0.000 left=0.124450000 right=0.124450000
OAR-PROBE azimuth=-30.000 gain_db=0.000 left=0.000000000 right=0.176000000
OAR-PROBE azimuth=0.000 gain_db=-6.000 left=0.062370000 right=0.062370000
"""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        a = root / "aurora.log"
        o = root / "oar.log"
        report = root / "report.json"
        a.write_text(aurora_text, encoding="utf-8")
        o.write_text(oar_text, encoding="utf-8")
        if analyze(a, o, report) != 0:
            raise AssertionError(report.read_text(encoding="utf-8"))
        payload = json.loads(report.read_text(encoding="utf-8"))
        if payload["verdict"] != "pass":
            raise AssertionError(payload)
    print("AURORA-OAR-OBJECT-DIFFERENTIAL-SELFTEST-PASS")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("self-test")
    analyze_parser = sub.add_parser("analyze")
    analyze_parser.add_argument("--aurora-log", type=Path, required=True)
    analyze_parser.add_argument("--oar-log", type=Path, required=True)
    analyze_parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "self-test":
        return self_test()
    return analyze(args.aurora_log, args.oar_log, args.output)


if __name__ == "__main__":
    raise SystemExit(main())
