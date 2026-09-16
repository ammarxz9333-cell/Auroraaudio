#!/usr/bin/env python3
"""Correlate one-listener GenAVB physical evidence without inventing proof.

The real gate consumes three independently captured JSON objects from one explicit
`epoch_id`: Aurora's NXP host probe, NXP gPTP before/after evidence, and ESP
listener before/after evidence. CI uses `--fixture-mode`; a fixture-mode success
is reported only as VALIDATOR-PASS and can never become PHYSICAL-PASS.
"""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

HOST_SCHEMA = "aurora.genavb.single-listener-host-evidence.v1"
NXP_GPTP_SCHEMA = "aurora.genavb.nxp-gptp-evidence.v1"
ESP_SCHEMA = "aurora.genavb.esp-listener-evidence.v1"


@dataclass
class CorrelationReport:
    schema_version: int
    verdict: str
    physical_complete: bool
    fixture_mode: bool
    epoch_id: str | None
    stream_id: str | None
    grandmaster_id: str | None
    esp_rx_delta: int | None
    failures: list[str]
    truth_boundary: str


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


def require_str(obj: dict[str, Any], key: str, failures: list[str]) -> str | None:
    value = obj.get(key)
    if not isinstance(value, str) or not value:
        failures.append(f"{key} must be a non-empty string")
        return None
    return value


def require_int(obj: dict[str, Any], key: str, failures: list[str]) -> int | None:
    value = obj.get(key)
    if not is_int(value):
        failures.append(f"{key} must be an integer")
        return None
    return value


def require_bool(obj: dict[str, Any], key: str, failures: list[str]) -> bool | None:
    value = obj.get(key)
    if not isinstance(value, bool):
        failures.append(f"{key} must be a boolean")
        return None
    return value


def normalized_id(value: str | None) -> str | None:
    return value.lower() if value is not None else None


def correlate(
    host: dict[str, Any],
    nxp: dict[str, Any],
    esp: dict[str, Any],
    *,
    fixture_mode: bool,
) -> CorrelationReport:
    failures: list[str] = []

    if host.get("schema") != HOST_SCHEMA:
        failures.append(f"host schema must be {HOST_SCHEMA}")
    if nxp.get("schema") != NXP_GPTP_SCHEMA:
        failures.append(f"NXP gPTP schema must be {NXP_GPTP_SCHEMA}")
    if esp.get("schema") != ESP_SCHEMA:
        failures.append(f"ESP schema must be {ESP_SCHEMA}")

    if host.get("verdict") != "HOST_PASS":
        failures.append("host verdict must be HOST_PASS")
    if host.get("physical_complete") is not False:
        failures.append("host evidence must remain physical_complete=false")
    if nxp.get("verdict") != "PASS":
        failures.append("NXP gPTP verdict must be PASS")
    if esp.get("verdict") != "PASS":
        failures.append("ESP listener verdict must be PASS")

    host_epoch = require_str(host, "epoch_id", failures)
    nxp_epoch = require_str(nxp, "epoch_id", failures)
    esp_epoch = require_str(esp, "epoch_id", failures)
    if host_epoch is not None and (nxp_epoch != host_epoch or esp_epoch != host_epoch):
        failures.append("epoch_id mismatch across host/NXP/ESP evidence")

    host_stream = normalized_id(require_str(host, "stream_id", failures))
    esp_stream = normalized_id(require_str(esp, "stream_id", failures))
    if host_stream is not None and esp_stream != host_stream:
        failures.append("stream_id mismatch between AVDECC host CONNECT and ESP listener")

    for key, expected in (
        ("sample_rate_hz", 48_000),
        ("channels", 2),
        ("bit_depth", 24),
        ("block_frames", 48),
    ):
        value = require_int(host, key, failures)
        if value is not None and value != expected:
            failures.append(f"host {key} must be {expected}, got {value}")

    for key, expected in (("sample_rate_hz", 48_000), ("channels", 2), ("bit_depth", 24)):
        value = require_int(esp, key, failures)
        if value is not None and value != expected:
            failures.append(f"ESP {key} must be {expected}, got {value}")

    blocks = require_int(host, "blocks_submitted", failures)
    if blocks is not None and blocks <= 0:
        failures.append("host blocks_submitted must be positive")

    host_started = require_int(host, "host_started_unix_ms", failures)
    connect_at = require_int(host, "connect_unix_ms", failures)
    send_started = require_int(host, "send_started_unix_ms", failures)
    send_ended = require_int(host, "send_ended_unix_ms", failures)
    if None not in (host_started, connect_at, send_started, send_ended):
        assert host_started is not None
        assert connect_at is not None
        assert send_started is not None
        assert send_ended is not None
        if not host_started <= connect_at <= send_started < send_ended:
            failures.append("host timestamps are not ordered host_start <= connect <= send_start < send_end")

    nxp_before = require_int(nxp, "before_unix_ms", failures)
    nxp_after = require_int(nxp, "after_unix_ms", failures)
    esp_before = require_int(esp, "before_unix_ms", failures)
    esp_after = require_int(esp, "after_unix_ms", failures)

    nxp_locked_before = require_bool(nxp, "locked_before", failures)
    nxp_locked_after = require_bool(nxp, "locked_after", failures)
    esp_locked_before = require_bool(esp, "gptp_locked_before", failures)
    esp_locked_after = require_bool(esp, "gptp_locked_after", failures)
    if nxp_locked_before is False or nxp_locked_after is False:
        failures.append("NXP gPTP must be locked before and after the send interval")
    if esp_locked_before is False or esp_locked_after is False:
        failures.append("ESP gPTP must be locked before and after the send interval")

    nxp_gm_before = normalized_id(require_str(nxp, "grandmaster_id_before", failures))
    nxp_gm_after = normalized_id(require_str(nxp, "grandmaster_id_after", failures))
    esp_gm_before = normalized_id(require_str(esp, "grandmaster_id_before", failures))
    esp_gm_after = normalized_id(require_str(esp, "grandmaster_id_after", failures))
    gm_values = [nxp_gm_before, nxp_gm_after, esp_gm_before, esp_gm_after]
    grandmaster_id: str | None = None
    if all(value is not None for value in gm_values):
        grandmaster_id = nxp_gm_before
        if len(set(gm_values)) != 1:
            failures.append("NXP and ESP must remain locked to the same grandmaster across the epoch")

    acmp_before = require_bool(esp, "acmp_connected_before", failures)
    acmp_after = require_bool(esp, "acmp_connected_after", failures)
    if acmp_before is False or acmp_after is False:
        failures.append("ESP listener must remain ACMP-connected before and after the send interval")

    rx_before = require_int(esp, "rx_counter_before", failures)
    rx_after = require_int(esp, "rx_counter_after", failures)
    esp_rx_delta: int | None = None
    if rx_before is not None and rx_after is not None:
        if rx_before < 0 or rx_after < 0:
            failures.append("ESP receive counters must be non-negative")
        elif rx_after <= rx_before:
            failures.append("ESP STREAM_INPUT receive counter did not increase during the epoch")
        else:
            esp_rx_delta = rx_after - rx_before

    last_rx = require_int(esp, "last_rx_us_after", failures)
    if last_rx is not None and last_rx <= 0:
        failures.append("ESP last_rx_us_after must show an observed stream frame")

    if send_started is not None and send_ended is not None:
        if nxp_before is not None and nxp_after is not None:
            if not (nxp_before <= send_started and nxp_after >= send_ended):
                failures.append("NXP gPTP before/after samples do not bracket the host send interval")
        if esp_before is not None and esp_after is not None:
            if not (esp_before <= send_started and esp_after >= send_ended):
                failures.append("ESP listener before/after samples do not bracket the host send interval")

    if nxp_before is not None and nxp_after is not None and nxp_after <= nxp_before:
        failures.append("NXP gPTP after timestamp must be later than before timestamp")
    if esp_before is not None and esp_after is not None and esp_after <= esp_before:
        failures.append("ESP after timestamp must be later than before timestamp")

    if failures:
        verdict = "FAIL"
        physical_complete = False
    elif fixture_mode:
        verdict = "VALIDATOR-PASS"
        physical_complete = False
    else:
        verdict = "PHYSICAL-PASS"
        physical_complete = True

    return CorrelationReport(
        schema_version=1,
        verdict=verdict,
        physical_complete=physical_complete,
        fixture_mode=fixture_mode,
        epoch_id=host_epoch,
        stream_id=host_stream,
        grandmaster_id=grandmaster_id,
        esp_rx_delta=esp_rx_delta,
        failures=failures,
        truth_boundary=(
            "fixture-mode success proves only correlator logic; PHYSICAL-PASS requires real, same-epoch NXP/ESP captures"
        ),
    )


def positive_fixture() -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    epoch = "20260916T193000Z-run01"
    stream_id = "02:00:00:00:00:01:00:0a"
    gm = "02:00:00:ff:fe:00:00:01"
    host = {
        "schema": HOST_SCHEMA,
        "verdict": "HOST_PASS",
        "physical_complete": False,
        "epoch_id": epoch,
        "host_started_unix_ms": 1_000,
        "connect_unix_ms": 1_100,
        "send_started_unix_ms": 1_200,
        "send_ended_unix_ms": 6_200,
        "stream_id": stream_id,
        "sample_rate_hz": 48_000,
        "channels": 2,
        "bit_depth": 24,
        "block_frames": 48,
        "blocks_submitted": 5_000,
    }
    nxp = {
        "schema": NXP_GPTP_SCHEMA,
        "verdict": "PASS",
        "epoch_id": epoch,
        "before_unix_ms": 1_150,
        "after_unix_ms": 6_250,
        "locked_before": True,
        "locked_after": True,
        "grandmaster_id_before": gm,
        "grandmaster_id_after": gm,
    }
    esp = {
        "schema": ESP_SCHEMA,
        "verdict": "PASS",
        "epoch_id": epoch,
        "stream_id": stream_id,
        "sample_rate_hz": 48_000,
        "channels": 2,
        "bit_depth": 24,
        "before_unix_ms": 1_150,
        "after_unix_ms": 6_250,
        "acmp_connected_before": True,
        "acmp_connected_after": True,
        "gptp_locked_before": True,
        "gptp_locked_after": True,
        "grandmaster_id_before": gm,
        "grandmaster_id_after": gm,
        "rx_counter_before": 10_000,
        "rx_counter_after": 15_000,
        "last_rx_us_after": 9_999_999,
    }
    return host, nxp, esp


def self_test() -> int:
    host, nxp, esp = positive_fixture()
    report = correlate(host, nxp, esp, fixture_mode=True)
    assert report.verdict == "VALIDATOR-PASS"
    assert report.physical_complete is False
    assert report.esp_rx_delta == 5_000

    broken = [
        ("epoch", lambda h, n, e: e.__setitem__("epoch_id", "other-run")),
        ("stream", lambda h, n, e: e.__setitem__("stream_id", "00:00:00:00:00:00:00:00")),
        ("rx", lambda h, n, e: e.__setitem__("rx_counter_after", e["rx_counter_before"])),
        ("nxp-lock", lambda h, n, e: n.__setitem__("locked_after", False)),
        ("esp-lock", lambda h, n, e: e.__setitem__("gptp_locked_after", False)),
        ("grandmaster", lambda h, n, e: e.__setitem__("grandmaster_id_after", "different-gm")),
        ("window", lambda h, n, e: e.__setitem__("after_unix_ms", h["send_ended_unix_ms"] - 1)),
    ]
    for name, mutate in broken:
        host, nxp, esp = positive_fixture()
        mutate(host, nxp, esp)
        report = correlate(host, nxp, esp, fixture_mode=True)
        assert report.verdict == "FAIL", name
        assert report.physical_complete is False, name

    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        host, nxp, esp = positive_fixture()
        for name, value in (("host.json", host), ("nxp.json", nxp), ("esp.json", esp)):
            (base / name).write_text(json.dumps(value), encoding="utf-8")
        loaded = [read_json(base / name) for name in ("host.json", "nxp.json", "esp.json")]
        report = correlate(*loaded, fixture_mode=True)
        assert report.verdict == "VALIDATOR-PASS"

    print("aurora-genavb-single-listener-evidence: SELF-TEST PASS positive=1 negative=7 fixture-physical-pass=forbidden")
    return 0


def command_correlate(args: argparse.Namespace) -> int:
    try:
        report = correlate(
            read_json(args.host),
            read_json(args.nxp_gptp),
            read_json(args.esp),
            fixture_mode=args.fixture_mode,
        )
    except ValueError as exc:
        report = CorrelationReport(
            schema_version=1,
            verdict="FAIL",
            physical_complete=False,
            fixture_mode=args.fixture_mode,
            epoch_id=None,
            stream_id=None,
            grandmaster_id=None,
            esp_rx_delta=None,
            failures=[str(exc)],
            truth_boundary="malformed evidence fails closed",
        )

    payload = json.dumps(asdict(report), sort_keys=True, indent=2)
    print(payload)
    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(payload + "\n", encoding="utf-8")
    return 0 if report.verdict != "FAIL" else 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    correlate_parser = subparsers.add_parser("correlate")
    correlate_parser.add_argument("--host", type=Path, required=True)
    correlate_parser.add_argument("--nxp-gptp", type=Path, required=True)
    correlate_parser.add_argument("--esp", type=Path, required=True)
    correlate_parser.add_argument("--output", type=Path)
    correlate_parser.add_argument(
        "--fixture-mode",
        action="store_true",
        help="validate synthetic fixtures without ever emitting PHYSICAL-PASS",
    )
    correlate_parser.set_defaults(func=command_correlate)

    self_test_parser = subparsers.add_parser("self-test")
    self_test_parser.set_defaults(func=lambda _args: self_test())
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
