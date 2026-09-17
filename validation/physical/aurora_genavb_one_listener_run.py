#!/usr/bin/env python3
"""Bundle one-listener GenAVB physical evidence in one fail-closed command.

This runner does not capture hardware by itself. It consumes the independently
captured host, NXP gPTP and ESP listener snapshots, builds the two intermediate
evidence objects and invokes Aurora's final one-listener correlator.

Without --fixture-mode, a successful final correlation is allowed to report
PHYSICAL-PASS only because the underlying correlator requires real evidence
contracts and marks fixture-mode runs as non-physical.
"""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from dataclasses import asdict
from pathlib import Path
from typing import Any

from aurora_esp_avb_listener_evidence import (
    build_evidence as build_esp_evidence,
    fixture as esp_fixture,
)
from aurora_genavb_nxp_gptp_evidence import (
    build_evidence as build_nxp_evidence,
    positive_snapshot as nxp_positive_snapshot,
)
from aurora_genavb_single_listener_evidence import (
    correlate,
    positive_fixture,
)


def read_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read JSON {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise ValueError(f"JSON root must be an object: {path}")
    return value


def write_object(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n", encoding="utf-8")


def bundle(
    *,
    host_path: Path,
    nxp_before_path: Path,
    nxp_after_path: Path,
    esp_before_path: Path,
    esp_after_path: Path,
    epoch_id: str,
    output_dir: Path,
    fixture_mode: bool,
) -> dict[str, Any]:
    host = read_object(host_path)
    nxp_before = read_object(nxp_before_path)
    nxp_after = read_object(nxp_after_path)
    esp_before = read_object(esp_before_path)
    esp_after = read_object(esp_after_path)

    nxp_evidence = build_nxp_evidence(nxp_before, nxp_after, epoch_id)
    write_object(output_dir / "nxp-gptp-evidence.json", nxp_evidence)

    try:
        esp_evidence = build_esp_evidence(esp_before, esp_after)
    except ValueError as exc:
        esp_evidence = {
            "schema": "aurora.genavb.esp-listener-evidence.v1",
            "verdict": "FAIL",
            "physical_complete": False,
            "epoch_id": epoch_id,
            "failure": str(exc),
        }
    write_object(output_dir / "esp-listener-evidence.json", esp_evidence)

    report = correlate(host, nxp_evidence, esp_evidence, fixture_mode=fixture_mode)
    report_dict = asdict(report)
    write_object(output_dir / "one-listener-correlation.json", report_dict)

    manifest = {
        "schema": "aurora.genavb.one-listener-run.v1",
        "verdict": report.verdict,
        "physical_complete": report.physical_complete,
        "fixture_mode": fixture_mode,
        "epoch_id": report.epoch_id,
        "stream_id": report.stream_id,
        "grandmaster_id": report.grandmaster_id,
        "esp_rx_delta": report.esp_rx_delta,
        "artifacts": {
            "host": str(host_path),
            "nxp_before": str(nxp_before_path),
            "nxp_after": str(nxp_after_path),
            "esp_before": str(esp_before_path),
            "esp_after": str(esp_after_path),
            "nxp_evidence": str(output_dir / "nxp-gptp-evidence.json"),
            "esp_evidence": str(output_dir / "esp-listener-evidence.json"),
            "correlation": str(output_dir / "one-listener-correlation.json"),
        },
        "failures": report.failures,
        "truth_boundary": (
            "This runner only bundles independently captured evidence. "
            "Fixture-mode can never produce PHYSICAL-PASS; real hardware capture remains required."
        ),
    }
    write_object(output_dir / "run-manifest.json", manifest)
    return manifest


def self_test() -> int:
    host, _, _ = positive_fixture()
    epoch = str(host["epoch_id"])

    nxp_before = nxp_positive_snapshot(1_150)
    nxp_after = nxp_positive_snapshot(6_250)

    esp_before = esp_fixture(10_000, 1_150, 900_000)
    esp_after = esp_fixture(15_000, 6_250, 6_200_000)
    esp_before["epoch_id"] = epoch
    esp_after["epoch_id"] = epoch
    esp_before["stream_id"] = host["stream_id"]
    esp_after["stream_id"] = host["stream_id"]

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        inputs = root / "inputs"
        outputs = root / "outputs"
        inputs.mkdir()

        values = {
            "host.json": host,
            "nxp-before.json": nxp_before,
            "nxp-after.json": nxp_after,
            "esp-before.json": esp_before,
            "esp-after.json": esp_after,
        }
        for name, value in values.items():
            write_object(inputs / name, value)

        result = bundle(
            host_path=inputs / "host.json",
            nxp_before_path=inputs / "nxp-before.json",
            nxp_after_path=inputs / "nxp-after.json",
            esp_before_path=inputs / "esp-before.json",
            esp_after_path=inputs / "esp-after.json",
            epoch_id=epoch,
            output_dir=outputs,
            fixture_mode=True,
        )
        assert result["verdict"] == "VALIDATOR-PASS"
        assert result["physical_complete"] is False
        assert result["esp_rx_delta"] == 5_000
        for name in (
            "nxp-gptp-evidence.json",
            "esp-listener-evidence.json",
            "one-listener-correlation.json",
            "run-manifest.json",
        ):
            assert (outputs / name).is_file(), name

        broken_after = dict(esp_after)
        broken_after["rx_counter"] = esp_before["rx_counter"]
        write_object(inputs / "esp-after-broken.json", broken_after)
        broken = bundle(
            host_path=inputs / "host.json",
            nxp_before_path=inputs / "nxp-before.json",
            nxp_after_path=inputs / "nxp-after.json",
            esp_before_path=inputs / "esp-before.json",
            esp_after_path=inputs / "esp-after-broken.json",
            epoch_id=epoch,
            output_dir=root / "broken-output",
            fixture_mode=True,
        )
        assert broken["verdict"] == "FAIL"
        assert broken["physical_complete"] is False

    print("aurora-genavb-one-listener-run: SELF-TEST PASS positive=1 negative=1 fixture-physical-pass=forbidden")
    return 0


def command_bundle(args: argparse.Namespace) -> int:
    try:
        result = bundle(
            host_path=args.host,
            nxp_before_path=args.nxp_before,
            nxp_after_path=args.nxp_after,
            esp_before_path=args.esp_before,
            esp_after_path=args.esp_after,
            epoch_id=args.epoch_id,
            output_dir=args.output_dir,
            fixture_mode=args.fixture_mode,
        )
    except ValueError as exc:
        failure = {
            "schema": "aurora.genavb.one-listener-run.v1",
            "verdict": "FAIL",
            "physical_complete": False,
            "fixture_mode": args.fixture_mode,
            "epoch_id": args.epoch_id,
            "failures": [str(exc)],
            "truth_boundary": "Malformed or unreadable evidence fails closed.",
        }
        write_object(args.output_dir / "run-manifest.json", failure)
        print(json.dumps(failure, sort_keys=True, indent=2))
        return 1

    print(json.dumps(result, sort_keys=True, indent=2))
    return 0 if result["verdict"] != "FAIL" else 1


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    sub = root.add_subparsers(dest="command", required=True)

    run = sub.add_parser("bundle")
    run.add_argument("--host", type=Path, required=True)
    run.add_argument("--nxp-before", type=Path, required=True)
    run.add_argument("--nxp-after", type=Path, required=True)
    run.add_argument("--esp-before", type=Path, required=True)
    run.add_argument("--esp-after", type=Path, required=True)
    run.add_argument("--epoch-id", required=True)
    run.add_argument("--output-dir", type=Path, required=True)
    run.add_argument(
        "--fixture-mode",
        action="store_true",
        help="mark the run as validator-only; PHYSICAL-PASS is forbidden",
    )
    run.set_defaults(func=command_bundle)

    test = sub.add_parser("self-test")
    test.set_defaults(func=lambda _args: self_test())
    return root


def main() -> int:
    args = parser().parse_args()
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
