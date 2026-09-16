#!/usr/bin/env python3
"""Build fail-closed ESP listener evidence from two endpoint snapshots.

The input snapshots come from Aurora's narrow ESP-IDF evidence component linked
against the exact-pinned esp_avb/esp_ptp runtime. The output matches the ESP
schema consumed by the one-listener NXP/ESP physical correlator.
"""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from pathlib import Path
from typing import Any

SNAPSHOT_SCHEMA = "aurora.genavb.esp-listener-snapshot.v1"
EVIDENCE_SCHEMA = "aurora.genavb.esp-listener-evidence.v1"
EXPECTED_RATE = 48_000
EXPECTED_CHANNELS = 2
EXPECTED_BIT_DEPTH = 24


def read_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read JSON {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise ValueError(f"JSON root must be an object: {path}")
    return value


def is_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def valid_id(value: Any) -> bool:
    if not isinstance(value, str) or not value:
        return False
    parts = value.lower().split(":")
    if len(parts) != 8:
        return False
    try:
        octets = [int(part, 16) for part in parts]
    except ValueError:
        return False
    return all(0 <= octet <= 255 for octet in octets) and any(octets)


def validate_snapshot(snapshot: dict[str, Any], label: str, failures: list[str]) -> None:
    if snapshot.get("schema") != SNAPSHOT_SCHEMA:
        failures.append(f"{label} schema must be {SNAPSHOT_SCHEMA}")
    if snapshot.get("verdict") != "PASS":
        failures.append(f"{label} snapshot verdict must be PASS")
    if snapshot.get("acmp_connected") is not True:
        failures.append(f"{label} listener must be ACMP-connected")
    if snapshot.get("gptp_locked") is not True:
        failures.append(f"{label} gPTP must be locked")
    if snapshot.get("sample_rate_hz") != EXPECTED_RATE:
        failures.append(f"{label} sample rate must be {EXPECTED_RATE}")
    if snapshot.get("channels") != EXPECTED_CHANNELS:
        failures.append(f"{label} channels must be {EXPECTED_CHANNELS}")
    if snapshot.get("bit_depth") != EXPECTED_BIT_DEPTH:
        failures.append(f"{label} bit depth must be {EXPECTED_BIT_DEPTH}")
    for key in ("entity_id", "stream_id", "grandmaster_id"):
        if not valid_id(snapshot.get(key)):
            failures.append(f"{label} {key} must be a non-zero 8-octet identity")
    for key in ("sample_unix_ms", "rx_frames", "last_rx_us", "last_sync_monotonic_ms"):
        if not is_int(snapshot.get(key)):
            failures.append(f"{label} {key} must be an integer")
    if is_int(snapshot.get("sample_unix_ms")) and snapshot["sample_unix_ms"] <= 0:
        failures.append(f"{label} sample_unix_ms must be positive")
    if is_int(snapshot.get("rx_frames")) and snapshot["rx_frames"] < 0:
        failures.append(f"{label} rx_frames must be non-negative")
    if is_int(snapshot.get("last_sync_monotonic_ms")) and snapshot["last_sync_monotonic_ms"] <= 0:
        failures.append(f"{label} last_sync_monotonic_ms must be positive")


def build_evidence(
    before: dict[str, Any], after: dict[str, Any], epoch_id: str
) -> dict[str, Any]:
    failures: list[str] = []
    if not epoch_id or len(epoch_id) > 64 or any(
        not (char.isalnum() or char in "-_.") for char in epoch_id
    ):
        failures.append("epoch_id must be 1-64 alphanumeric/-/_/. characters")

    validate_snapshot(before, "before", failures)
    validate_snapshot(after, "after", failures)

    for key in ("entity_id", "stream_id", "grandmaster_id"):
        left = before.get(key)
        right = after.get(key)
        if isinstance(left, str) and isinstance(right, str) and left.lower() != right.lower():
            failures.append(f"{key} changed between before and after snapshots")

    for key in ("sample_rate_hz", "channels", "bit_depth"):
        if before.get(key) != after.get(key):
            failures.append(f"{key} changed between before and after snapshots")

    before_ms = before.get("sample_unix_ms")
    after_ms = after.get("sample_unix_ms")
    if is_int(before_ms) and is_int(after_ms) and after_ms <= before_ms:
        failures.append("after snapshot timestamp must be later than before snapshot")

    rx_before = before.get("rx_frames")
    rx_after = after.get("rx_frames")
    if is_int(rx_before) and is_int(rx_after) and rx_after <= rx_before:
        failures.append("ESP frames_rx counter did not increase during the epoch")

    last_rx_before = before.get("last_rx_us")
    last_rx_after = after.get("last_rx_us")
    if is_int(last_rx_before) and is_int(last_rx_after):
        if last_rx_after <= 0:
            failures.append("ESP last_rx_us_after must show an observed AVTP frame")
        elif last_rx_after <= last_rx_before:
            failures.append("ESP last_rx_us did not advance during the epoch")

    return {
        "schema": EVIDENCE_SCHEMA,
        "verdict": "PASS" if not failures else "FAIL",
        "epoch_id": epoch_id,
        "entity_id": after.get("entity_id"),
        "stream_id": after.get("stream_id"),
        "sample_rate_hz": after.get("sample_rate_hz"),
        "channels": after.get("channels"),
        "bit_depth": after.get("bit_depth"),
        "before_unix_ms": before_ms,
        "after_unix_ms": after_ms,
        "acmp_connected_before": before.get("acmp_connected") is True,
        "acmp_connected_after": after.get("acmp_connected") is True,
        "gptp_locked_before": before.get("gptp_locked") is True,
        "gptp_locked_after": after.get("gptp_locked") is True,
        "grandmaster_id_before": before.get("grandmaster_id"),
        "grandmaster_id_after": after.get("grandmaster_id"),
        "rx_counter_before": rx_before,
        "rx_counter_after": rx_after,
        "last_rx_us_before": last_rx_before,
        "last_rx_us_after": last_rx_after,
        "last_sync_monotonic_ms_before": before.get("last_sync_monotonic_ms"),
        "last_sync_monotonic_ms_after": after.get("last_sync_monotonic_ms"),
        "failures": failures,
        "truth_boundary": (
            "PASS proves only one ESP listener remained connected/clock-valid and received new AVTP frames; "
            "the complete physical verdict also requires correlated NXP host and NXP gPTP evidence"
        ),
    }


def positive_snapshot(sample_ms: int, rx_frames: int, last_rx_us: int) -> dict[str, Any]:
    return {
        "schema": SNAPSHOT_SCHEMA,
        "verdict": "PASS",
        "sample_unix_ms": sample_ms,
        "entity_id": "02:00:00:00:00:20:00:01",
        "stream_id": "02:00:00:00:00:01:00:0a",
        "grandmaster_id": "02:00:00:ff:fe:00:00:01",
        "acmp_connected": True,
        "gptp_locked": True,
        "sample_rate_hz": EXPECTED_RATE,
        "channels": EXPECTED_CHANNELS,
        "bit_depth": EXPECTED_BIT_DEPTH,
        "rx_frames": rx_frames,
        "last_rx_us": last_rx_us,
        "last_sync_monotonic_ms": 5_000,
    }


def self_test() -> int:
    before = positive_snapshot(1_000, 100, 10_000)
    after = positive_snapshot(7_000, 5_100, 6_010_000)
    report = build_evidence(before, after, "epoch-01")
    assert report["verdict"] == "PASS"
    assert report["rx_counter_after"] - report["rx_counter_before"] == 5_000

    scenarios = {
        "disconnected": lambda b, a: a.update(acmp_connected=False, verdict="FAIL"),
        "gptp": lambda b, a: a.update(gptp_locked=False, verdict="FAIL"),
        "stream": lambda b, a: a.update(stream_id="02:00:00:00:00:02:00:0a"),
        "gm": lambda b, a: a.update(grandmaster_id="02:00:00:ff:fe:00:00:02"),
        "rx": lambda b, a: a.update(rx_frames=b["rx_frames"]),
        "last-rx": lambda b, a: a.update(last_rx_us=b["last_rx_us"]),
        "format": lambda b, a: a.update(bit_depth=32),
        "time": lambda b, a: a.update(sample_unix_ms=b["sample_unix_ms"]),
    }
    for name, mutate in scenarios.items():
        before = positive_snapshot(1_000, 100, 10_000)
        after = positive_snapshot(7_000, 5_100, 6_010_000)
        mutate(before, after)
        report = build_evidence(before, after, "epoch-01")
        assert report["verdict"] == "FAIL", name

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        before = positive_snapshot(1_000, 100, 10_000)
        after = positive_snapshot(7_000, 5_100, 6_010_000)
        (root / "before.json").write_text(json.dumps(before), encoding="utf-8")
        (root / "after.json").write_text(json.dumps(after), encoding="utf-8")
        report = build_evidence(
            read_object(root / "before.json"),
            read_object(root / "after.json"),
            "epoch-02",
        )
        assert report["verdict"] == "PASS"

    print("aurora-genavb-esp-listener-evidence: SELF-TEST PASS positive=1 negative=8")
    return 0


def command_build(args: argparse.Namespace) -> int:
    try:
        report = build_evidence(read_object(args.before), read_object(args.after), args.epoch_id)
    except ValueError as exc:
        report = {
            "schema": EVIDENCE_SCHEMA,
            "verdict": "FAIL",
            "epoch_id": args.epoch_id,
            "failures": [str(exc)],
            "truth_boundary": "malformed ESP snapshot evidence fails closed",
        }

    payload = json.dumps(report, sort_keys=True, indent=2)
    print(payload)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(payload + "\n", encoding="utf-8")
    return 0 if report.get("verdict") == "PASS" else 1


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    sub = root.add_subparsers(dest="command", required=True)

    build = sub.add_parser("build")
    build.add_argument("--before", type=Path, required=True)
    build.add_argument("--after", type=Path, required=True)
    build.add_argument("--epoch-id", required=True)
    build.add_argument("--output", type=Path)
    build.set_defaults(func=command_build)

    test = sub.add_parser("self-test")
    test.set_defaults(func=lambda _args: self_test())
    return root


def main() -> int:
    args = parser().parse_args()
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
