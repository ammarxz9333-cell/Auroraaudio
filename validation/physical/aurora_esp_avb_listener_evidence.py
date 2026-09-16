#!/usr/bin/env python3
"""Build fail-closed ESP-AVB listener evidence from two machine-readable snapshots.

The input snapshot contract is intentionally small enough to be emitted by a
pinned ESP firmware instrumentation path while preserving the facts required by
Aurora's one-listener physical correlator. A successful build proves only that
the two ESP snapshots are internally consistent; PHYSICAL-PASS remains the job
of aurora_genavb_single_listener_evidence.py with real NXP/ESP captures.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from pathlib import Path
from typing import Any

SNAPSHOT_SCHEMA = "aurora.esp-avb.listener-snapshot.v1"
EVIDENCE_SCHEMA = "aurora.genavb.esp-listener-evidence.v1"
EPOCH_RE = re.compile(r"^[A-Za-z0-9._-]{1,64}$")
ID_RE = re.compile(r"^(?:[0-9A-Fa-f]{2}:){7}[0-9A-Fa-f]{2}$")


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read JSON {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise ValueError(f"JSON root must be an object: {path}")
    return value


def is_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def require_int(obj: dict[str, Any], key: str) -> int:
    value = obj.get(key)
    if not is_int(value):
        raise ValueError(f"{key} must be an integer")
    return value


def require_bool(obj: dict[str, Any], key: str) -> bool:
    value = obj.get(key)
    if not isinstance(value, bool):
        raise ValueError(f"{key} must be a boolean")
    return value


def require_str(obj: dict[str, Any], key: str) -> str:
    value = obj.get(key)
    if not isinstance(value, str) or not value:
        raise ValueError(f"{key} must be a non-empty string")
    return value


def normalize_id(value: str, field: str) -> str:
    if not ID_RE.fullmatch(value):
        raise ValueError(f"{field} must be eight colon-separated octets")
    return value.lower()


def validate_snapshot(snapshot: dict[str, Any]) -> dict[str, Any]:
    if snapshot.get("schema") != SNAPSHOT_SCHEMA:
        raise ValueError(f"snapshot schema must be {SNAPSHOT_SCHEMA}")
    if snapshot.get("verdict") != "PASS":
        raise ValueError("snapshot verdict must be PASS")

    epoch = require_str(snapshot, "epoch_id")
    if not EPOCH_RE.fullmatch(epoch):
        raise ValueError("epoch_id has invalid characters or length")

    capture = require_int(snapshot, "capture_unix_ms")
    if capture <= 0:
        raise ValueError("capture_unix_ms must be positive")

    stream_id = normalize_id(require_str(snapshot, "stream_id"), "stream_id")
    gm = normalize_id(require_str(snapshot, "grandmaster_id"), "grandmaster_id")
    if gm == "00:00:00:00:00:00:00:00":
        raise ValueError("grandmaster_id must be non-zero")

    sample_rate = require_int(snapshot, "sample_rate_hz")
    channels = require_int(snapshot, "channels")
    bit_depth = require_int(snapshot, "bit_depth")
    if sample_rate != 48_000 or channels != 2 or bit_depth != 24:
        raise ValueError("listener media contract must be 48000 Hz / 2 channels / 24 bit")

    connected = require_bool(snapshot, "acmp_connected")
    locked = require_bool(snapshot, "gptp_locked")
    if not connected:
        raise ValueError("listener must be ACMP-connected")
    if not locked:
        raise ValueError("listener gPTP must be locked")

    rx_counter = require_int(snapshot, "rx_counter")
    last_rx_us = require_int(snapshot, "last_rx_us")
    if rx_counter < 0 or last_rx_us < 0:
        raise ValueError("receive counters/timestamps must be non-negative")

    return {
        "epoch_id": epoch,
        "capture_unix_ms": capture,
        "stream_id": stream_id,
        "grandmaster_id": gm,
        "sample_rate_hz": sample_rate,
        "channels": channels,
        "bit_depth": bit_depth,
        "acmp_connected": connected,
        "gptp_locked": locked,
        "rx_counter": rx_counter,
        "last_rx_us": last_rx_us,
    }


def build_evidence(before_raw: dict[str, Any], after_raw: dict[str, Any]) -> dict[str, Any]:
    before = validate_snapshot(before_raw)
    after = validate_snapshot(after_raw)

    if before["epoch_id"] != after["epoch_id"]:
        raise ValueError("before/after epoch_id mismatch")
    if before["stream_id"] != after["stream_id"]:
        raise ValueError("before/after stream_id mismatch")
    for key in ("sample_rate_hz", "channels", "bit_depth"):
        if before[key] != after[key]:
            raise ValueError(f"before/after {key} mismatch")
    if before["grandmaster_id"] != after["grandmaster_id"]:
        raise ValueError("grandmaster changed during ESP evidence interval")
    if after["capture_unix_ms"] <= before["capture_unix_ms"]:
        raise ValueError("after capture time must be later than before capture time")
    if after["rx_counter"] <= before["rx_counter"]:
        raise ValueError("ESP receive counter did not increase")
    if after["last_rx_us"] <= 0:
        raise ValueError("ESP last_rx_us does not show observed stream traffic")
    if after["last_rx_us"] < before["last_rx_us"]:
        raise ValueError("ESP last_rx_us moved backwards")

    return {
        "schema": EVIDENCE_SCHEMA,
        "verdict": "PASS",
        "physical_complete": False,
        "epoch_id": before["epoch_id"],
        "stream_id": before["stream_id"],
        "sample_rate_hz": before["sample_rate_hz"],
        "channels": before["channels"],
        "bit_depth": before["bit_depth"],
        "before_unix_ms": before["capture_unix_ms"],
        "after_unix_ms": after["capture_unix_ms"],
        "acmp_connected_before": before["acmp_connected"],
        "acmp_connected_after": after["acmp_connected"],
        "gptp_locked_before": before["gptp_locked"],
        "gptp_locked_after": after["gptp_locked"],
        "grandmaster_id_before": before["grandmaster_id"],
        "grandmaster_id_after": after["grandmaster_id"],
        "rx_counter_before": before["rx_counter"],
        "rx_counter_after": after["rx_counter"],
        "last_rx_us_before": before["last_rx_us"],
        "last_rx_us_after": after["last_rx_us"],
        "truth_boundary": (
            "PASS proves only internally consistent ESP before/after evidence; "
            "PHYSICAL-PASS requires correlation with real NXP host and gPTP evidence"
        ),
    }


def fixture(rx: int, capture: int, last_rx: int) -> dict[str, Any]:
    return {
        "schema": SNAPSHOT_SCHEMA,
        "verdict": "PASS",
        "epoch_id": "20260916T203000Z-run01",
        "capture_unix_ms": capture,
        "stream_id": "02:00:00:00:00:01:00:0a",
        "sample_rate_hz": 48_000,
        "channels": 2,
        "bit_depth": 24,
        "acmp_connected": True,
        "gptp_locked": True,
        "grandmaster_id": "02:00:00:ff:fe:00:00:01",
        "rx_counter": rx,
        "last_rx_us": last_rx,
    }


def self_test() -> int:
    before = fixture(10_000, 1_000, 900_000)
    after = fixture(15_000, 6_500, 6_400_000)
    evidence = build_evidence(before, after)
    assert evidence["verdict"] == "PASS"
    assert evidence["physical_complete"] is False
    assert evidence["rx_counter_after"] - evidence["rx_counter_before"] == 5_000

    failures = [
        lambda b, a: a.__setitem__("epoch_id", "other"),
        lambda b, a: a.__setitem__("stream_id", "02:00:00:00:00:02:00:0a"),
        lambda b, a: a.__setitem__("grandmaster_id", "02:00:00:ff:fe:00:00:02"),
        lambda b, a: a.__setitem__("rx_counter", b["rx_counter"]),
        lambda b, a: a.__setitem__("gptp_locked", False),
        lambda b, a: a.__setitem__("acmp_connected", False),
        lambda b, a: a.__setitem__("capture_unix_ms", b["capture_unix_ms"]),
        lambda b, a: a.__setitem__("last_rx_us", 0),
        lambda b, a: a.__setitem__("channels", 1),
    ]
    for index, mutate in enumerate(failures):
        b = fixture(10_000, 1_000, 900_000)
        a = fixture(15_000, 6_500, 6_400_000)
        mutate(b, a)
        try:
            build_evidence(b, a)
        except ValueError:
            pass
        else:
            raise AssertionError(f"negative fixture {index} unexpectedly passed")

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        before_path = root / "before.json"
        after_path = root / "after.json"
        before_path.write_text(json.dumps(before), encoding="utf-8")
        after_path.write_text(json.dumps(after), encoding="utf-8")
        loaded = build_evidence(read_json(before_path), read_json(after_path))
        assert loaded["stream_id"] == evidence["stream_id"]

    print("aurora-esp-avb-listener-evidence: SELF-TEST PASS positive=1 negative=9 physical-pass=forbidden")
    return 0


def command_build(args: argparse.Namespace) -> int:
    try:
        evidence = build_evidence(read_json(args.before), read_json(args.after))
    except ValueError as exc:
        payload = {
            "schema": EVIDENCE_SCHEMA,
            "verdict": "FAIL",
            "physical_complete": False,
            "failure": str(exc),
        }
        text = json.dumps(payload, sort_keys=True, indent=2)
        print(text)
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(text + "\n", encoding="utf-8")
        return 1

    text = json.dumps(evidence, sort_keys=True, indent=2)
    print(text)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text + "\n", encoding="utf-8")
    return 0


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    sub = root.add_subparsers(dest="command", required=True)
    build = sub.add_parser("build")
    build.add_argument("--before", type=Path, required=True)
    build.add_argument("--after", type=Path, required=True)
    build.add_argument("--output", type=Path)
    sub.add_parser("self-test")
    return root


def main() -> int:
    args = parser().parse_args()
    if args.command == "self-test":
        return self_test()
    if args.command == "build":
        return command_build(args)
    raise AssertionError(args.command)


if __name__ == "__main__":
    sys.exit(main())
