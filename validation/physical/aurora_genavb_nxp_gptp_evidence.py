#!/usr/bin/env python3
"""Build fail-closed NXP gPTP evidence from two exact-API snapshots.

This tool does not prove end-to-end physical interoperability. It only converts
before/after snapshots from `genavb_gptp_snapshot` into the NXP evidence schema
consumed by Aurora's one-listener physical correlator.
"""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from pathlib import Path
from typing import Any

SNAPSHOT_SCHEMA = "aurora.genavb.nxp-gptp-snapshot.v1"
EVIDENCE_SCHEMA = "aurora.genavb.nxp-gptp-evidence.v1"


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


def validate_snapshot(value: dict[str, Any], label: str, failures: list[str]) -> None:
    if value.get("schema") != SNAPSHOT_SCHEMA:
        failures.append(f"{label} schema must be {SNAPSHOT_SCHEMA}")
    if value.get("verdict") != "PASS":
        failures.append(f"{label} snapshot verdict must be PASS")
    if value.get("locked") is not True:
        failures.append(f"{label} snapshot must report locked=true")
    if value.get("clock_status") != "LOCKED":
        failures.append(f"{label} clock_status must be LOCKED")
    if value.get("source_type") != "INTERNAL":
        failures.append(f"{label} clock source must be INTERNAL")
    if value.get("source_local_id") != 1:
        failures.append(f"{label} local clock source must be PTP_CLK (1)")
    if not is_int(value.get("sample_unix_ms")) or value["sample_unix_ms"] <= 0:
        failures.append(f"{label} sample_unix_ms must be a positive integer")
    if not is_int(value.get("gptp_domain")):
        failures.append(f"{label} gptp_domain must be an integer")
    if not is_int(value.get("clock_domain")):
        failures.append(f"{label} clock_domain must be an integer")
    gm = value.get("grandmaster_id")
    if not isinstance(gm, str) or not gm or gm == "00:00:00:00:00:00:00:00":
        failures.append(f"{label} grandmaster_id must be non-zero")


def build_evidence(
    before: dict[str, Any], after: dict[str, Any], epoch_id: str
) -> dict[str, Any]:
    failures: list[str] = []
    if not epoch_id or any(ch.isspace() for ch in epoch_id):
        failures.append("epoch_id must be non-empty and contain no whitespace")

    validate_snapshot(before, "before", failures)
    validate_snapshot(after, "after", failures)

    before_ms = before.get("sample_unix_ms")
    after_ms = after.get("sample_unix_ms")
    if is_int(before_ms) and is_int(after_ms) and after_ms <= before_ms:
        failures.append("after snapshot timestamp must be later than before snapshot")

    before_gm = before.get("grandmaster_id")
    after_gm = after.get("grandmaster_id")
    if isinstance(before_gm, str) and isinstance(after_gm, str):
        if before_gm.lower() != after_gm.lower():
            failures.append("grandmaster changed between before and after snapshots")

    before_gptp_domain = before.get("gptp_domain")
    after_gptp_domain = after.get("gptp_domain")
    if is_int(before_gptp_domain) and is_int(after_gptp_domain):
        if before_gptp_domain != after_gptp_domain:
            failures.append("gPTP domain changed between snapshots")

    before_clock_domain = before.get("clock_domain")
    after_clock_domain = after.get("clock_domain")
    if is_int(before_clock_domain) and is_int(after_clock_domain):
        if before_clock_domain != after_clock_domain:
            failures.append("clock domain changed between snapshots")

    return {
        "schema": EVIDENCE_SCHEMA,
        "verdict": "PASS" if not failures else "FAIL",
        "epoch_id": epoch_id,
        "before_unix_ms": before_ms,
        "after_unix_ms": after_ms,
        "locked_before": before.get("locked") is True,
        "locked_after": after.get("locked") is True,
        "grandmaster_id_before": before_gm,
        "grandmaster_id_after": after_gm,
        "gptp_domain": before_gptp_domain,
        "clock_domain": before_clock_domain,
        "failures": failures,
        "truth_boundary": (
            "PASS proves only that the two NXP snapshots report one stable locked PTP clock epoch; "
            "the complete physical verdict additionally requires host and ESP evidence"
        ),
    }


def positive_snapshot(sample_unix_ms: int) -> dict[str, Any]:
    return {
        "schema": SNAPSHOT_SCHEMA,
        "verdict": "PASS",
        "sample_unix_ms": sample_unix_ms,
        "gptp_domain": 0,
        "clock_domain": 10,
        "grandmaster_id": "02:00:00:ff:fe:00:00:01",
        "clock_status": "LOCKED",
        "source_type": "INTERNAL",
        "source_local_id": 1,
        "locked": True,
    }


def self_test() -> int:
    before = positive_snapshot(1_000)
    after = positive_snapshot(7_000)
    report = build_evidence(before, after, "epoch-01")
    assert report["verdict"] == "PASS"

    scenarios = {
        "unlock": lambda b, a: a.update(locked=False, verdict="FAIL", clock_status="UNLOCKED"),
        "gm-change": lambda b, a: a.update(grandmaster_id="02:00:00:ff:fe:00:00:02"),
        "time-reverse": lambda b, a: a.update(sample_unix_ms=999),
        "gptp-domain": lambda b, a: a.update(gptp_domain=1),
        "clock-domain": lambda b, a: a.update(clock_domain=11),
        "wrong-source": lambda b, a: a.update(source_local_id=0),
    }
    for name, mutate in scenarios.items():
        before = positive_snapshot(1_000)
        after = positive_snapshot(7_000)
        mutate(before, after)
        report = build_evidence(before, after, "epoch-01")
        assert report["verdict"] == "FAIL", name

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        before = positive_snapshot(1_000)
        after = positive_snapshot(7_000)
        (root / "before.json").write_text(json.dumps(before), encoding="utf-8")
        (root / "after.json").write_text(json.dumps(after), encoding="utf-8")
        report = build_evidence(read_object(root / "before.json"), read_object(root / "after.json"), "epoch-02")
        assert report["verdict"] == "PASS"

    print("aurora-genavb-nxp-gptp-evidence: SELF-TEST PASS positive=1 negative=6")
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
            "truth_boundary": "malformed NXP snapshot evidence fails closed",
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
