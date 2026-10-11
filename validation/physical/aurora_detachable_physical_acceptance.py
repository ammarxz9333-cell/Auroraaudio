#!/usr/bin/env python3
"""Fail-closed physical acceptance for Aurora detachable speaker modules.

Consumes machine-readable electrical timing evidence from one or two real
speaker modules. A PASS is only possible when actual measured output timing,
PTP lock, stream progress, mute safety, and shared epoch/clock identity satisfy
the configured limits.

Synthetic fixtures are used only by self-test and never imply physical proof.
"""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from pathlib import Path
from typing import Any

MODULE_SCHEMA = "aurora.detachable-module-physical.v1"
PAIR_SCHEMA = "aurora.detachable-pair-physical.v1"


def is_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def read_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read JSON {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise ValueError(f"JSON root must be an object: {path}")
    return value


def req_str(obj: dict[str, Any], key: str) -> str:
    value = obj.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{key} must be a non-empty string")
    return value.strip()


def req_int(obj: dict[str, Any], key: str) -> int:
    value = obj.get(key)
    if not is_int(value):
        raise ValueError(f"{key} must be an integer")
    return value


def req_bool(obj: dict[str, Any], key: str) -> bool:
    value = obj.get(key)
    if not isinstance(value, bool):
        raise ValueError(f"{key} must be a boolean")
    return value


def validate_module(
    raw: dict[str, Any],
    *,
    max_start_error_us: int,
) -> dict[str, Any]:
    if raw.get("schema") != MODULE_SCHEMA:
        raise ValueError(f"schema must be {MODULE_SCHEMA}")
    if max_start_error_us < 0:
        raise ValueError("max_start_error_us must be non-negative")

    module_id = req_str(raw, "module_id")
    epoch_id = req_str(raw, "epoch_id")
    stream_id = req_str(raw, "stream_id")
    grandmaster_id = req_str(raw, "grandmaster_id")

    sample_rate_hz = req_int(raw, "sample_rate_hz")
    channels = req_int(raw, "channels")
    bit_depth = req_int(raw, "bit_depth")
    if (sample_rate_hz, channels, bit_depth) != (48_000, 2, 24):
        raise ValueError("media contract must be 48000 Hz / 2 channels / 24 bit")

    if not req_bool(raw, "gptp_locked"):
        raise ValueError("gPTP must remain locked")
    if not req_bool(raw, "acmp_connected"):
        raise ValueError("AVB listener must remain ACMP-connected")

    rx_before = req_int(raw, "rx_counter_before")
    rx_after = req_int(raw, "rx_counter_after")
    if rx_before < 0 or rx_after <= rx_before:
        raise ValueError("receive counter must increase during the trial")

    requested = req_int(raw, "requested_start_ptp_ns")
    measured = req_int(raw, "measured_start_ptp_ns")
    if requested <= 0 or measured <= 0:
        raise ValueError("requested/measured PTP start timestamps must be positive")

    pre_roll_samples = req_int(raw, "pre_roll_samples")
    if pre_roll_samples != 0:
        raise ValueError("electrical output became active before the requested handover boundary")

    underflows = req_int(raw, "underflows")
    if underflows != 0:
        raise ValueError("trial contains receiver underflows")

    reconnects = req_int(raw, "reconnects")
    if reconnects < 0:
        raise ValueError("reconnect count must be non-negative")

    if not req_bool(raw, "hardware_muted_before_start"):
        raise ValueError("hardware mute was not asserted before the activation boundary")
    if not req_bool(raw, "hardware_mute_on_stop"):
        raise ValueError("STOP/MUTE did not assert physical hardware mute")
    if not req_bool(raw, "hardware_mute_on_sync_loss"):
        raise ValueError("sync loss did not assert physical hardware mute")

    start_error_ns = measured - requested
    if abs(start_error_ns) > max_start_error_us * 1000:
        raise ValueError(
            f"activation error {start_error_ns / 1000.0:.3f} us exceeds "
            f"{max_start_error_us} us"
        )

    return {
        "schema": MODULE_SCHEMA,
        "verdict": "PASS",
        "physical_complete": True,
        "module_id": module_id,
        "epoch_id": epoch_id,
        "stream_id": stream_id,
        "grandmaster_id": grandmaster_id,
        "sample_rate_hz": sample_rate_hz,
        "channels": channels,
        "bit_depth": bit_depth,
        "requested_start_ptp_ns": requested,
        "measured_start_ptp_ns": measured,
        "start_error_ns": start_error_ns,
        "start_error_us": start_error_ns / 1000.0,
        "rx_counter_delta": rx_after - rx_before,
        "underflows": underflows,
        "reconnects": reconnects,
        "truth_boundary": (
            "PASS requires measured physical endpoint evidence for this module only; "
            "it does not prove a synchronized pair, RF robustness in other homes, "
            "acoustic localization, amplifier fidelity, or Dolby certification"
        ),
    }


def validate_pair(
    left_raw: dict[str, Any],
    right_raw: dict[str, Any],
    *,
    max_start_error_us: int,
    max_pair_skew_us: int,
) -> dict[str, Any]:
    if max_pair_skew_us < 0:
        raise ValueError("max_pair_skew_us must be non-negative")

    left = validate_module(left_raw, max_start_error_us=max_start_error_us)
    right = validate_module(right_raw, max_start_error_us=max_start_error_us)

    if left["module_id"] == right["module_id"]:
        raise ValueError("pair must contain two distinct module identities")
    if left["epoch_id"] != right["epoch_id"]:
        raise ValueError("modules were not tested in one shared handover epoch")
    if left["grandmaster_id"] != right["grandmaster_id"]:
        raise ValueError("modules are not locked to the same PTP grandmaster")
    if left["requested_start_ptp_ns"] != right["requested_start_ptp_ns"]:
        raise ValueError("modules were not armed for one shared absolute PTP start time")
    if left["stream_id"] == right["stream_id"]:
        raise ValueError("detached pair must use distinct AVB stream identities")

    skew_ns = right["measured_start_ptp_ns"] - left["measured_start_ptp_ns"]
    if abs(skew_ns) > max_pair_skew_us * 1000:
        raise ValueError(
            f"inter-module electrical skew {skew_ns / 1000.0:.3f} us exceeds "
            f"{max_pair_skew_us} us"
        )

    return {
        "schema": PAIR_SCHEMA,
        "verdict": "PHYSICAL-PASS",
        "physical_complete": True,
        "epoch_id": left["epoch_id"],
        "grandmaster_id": left["grandmaster_id"],
        "requested_start_ptp_ns": left["requested_start_ptp_ns"],
        "left": left,
        "right": right,
        "inter_module_skew_ns": skew_ns,
        "inter_module_skew_us": skew_ns / 1000.0,
        "limits": {
            "max_start_error_us": max_start_error_us,
            "max_pair_skew_us": max_pair_skew_us,
        },
        "truth_boundary": (
            "PHYSICAL-PASS proves only the captured two-module electrical timing trial "
            "under the declared thresholds; repeat trials and acoustic/RF acceptance "
            "remain separate gates"
        ),
    }


def fixture(
    module_id: str,
    stream_id: str,
    measured_start_ptp_ns: int,
) -> dict[str, Any]:
    return {
        "schema": MODULE_SCHEMA,
        "module_id": module_id,
        "epoch_id": "epoch-42",
        "stream_id": stream_id,
        "grandmaster_id": "02:00:00:ff:fe:00:00:01",
        "sample_rate_hz": 48_000,
        "channels": 2,
        "bit_depth": 24,
        "gptp_locked": True,
        "acmp_connected": True,
        "rx_counter_before": 10_000,
        "rx_counter_after": 30_000,
        "requested_start_ptp_ns": 2_000_000_000,
        "measured_start_ptp_ns": measured_start_ptp_ns,
        "pre_roll_samples": 0,
        "underflows": 0,
        "reconnects": 0,
        "hardware_muted_before_start": True,
        "hardware_mute_on_stop": True,
        "hardware_mute_on_sync_loss": True,
    }


def self_test() -> int:
    left = fixture("left-pod", "02:00:00:00:00:01:00:0a", 2_000_090_000)
    right = fixture("right-pod", "02:00:00:00:00:02:00:0a", 2_000_130_000)

    report = validate_pair(
        left,
        right,
        max_start_error_us=1000,
        max_pair_skew_us=1000,
    )
    assert report["verdict"] == "PHYSICAL-PASS"
    assert report["inter_module_skew_ns"] == 40_000

    negative = {
        "unlocked": lambda l, r: r.__setitem__("gptp_locked", False),
        "no-rx-progress": lambda l, r: r.__setitem__("rx_counter_after", r["rx_counter_before"]),
        "preroll": lambda l, r: r.__setitem__("pre_roll_samples", 1),
        "underflow": lambda l, r: r.__setitem__("underflows", 1),
        "missing-stop-mute": lambda l, r: r.__setitem__("hardware_mute_on_stop", False),
        "missing-sync-loss-mute": lambda l, r: r.__setitem__("hardware_mute_on_sync_loss", False),
        "different-epoch": lambda l, r: r.__setitem__("epoch_id", "epoch-43"),
        "different-gm": lambda l, r: r.__setitem__("grandmaster_id", "02:00:00:ff:fe:00:00:02"),
        "different-start": lambda l, r: r.__setitem__("requested_start_ptp_ns", 2_000_100_000),
        "same-stream": lambda l, r: r.__setitem__("stream_id", l["stream_id"]),
        "late-start": lambda l, r: r.__setitem__("measured_start_ptp_ns", 2_002_000_000),
        "pair-skew": lambda l, r: r.__setitem__("measured_start_ptp_ns", 2_001_500_000),
    }

    for name, mutate in negative.items():
        l = fixture("left-pod", "02:00:00:00:00:01:00:0a", 2_000_090_000)
        r = fixture("right-pod", "02:00:00:00:00:02:00:0a", 2_000_130_000)
        mutate(l, r)
        try:
            validate_pair(l, r, max_start_error_us=1000, max_pair_skew_us=1000)
        except ValueError:
            pass
        else:
            raise AssertionError(f"negative fixture unexpectedly passed: {name}")

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / "left.json").write_text(json.dumps(left), encoding="utf-8")
        (root / "right.json").write_text(json.dumps(right), encoding="utf-8")
        loaded = validate_pair(
            read_object(root / "left.json"),
            read_object(root / "right.json"),
            max_start_error_us=1000,
            max_pair_skew_us=1000,
        )
        assert loaded["verdict"] == "PHYSICAL-PASS"

    print(
        "aurora-detachable-physical-acceptance: SELF-TEST PASS "
        "positive=1 negative=12 physical-pass=synthetic-only"
    )
    return 0


def emit(report: dict[str, Any], output: Path | None) -> None:
    payload = json.dumps(report, sort_keys=True, indent=2)
    print(payload)
    if output:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(payload + "\n", encoding="utf-8")


def command_module(args: argparse.Namespace) -> int:
    try:
        report = validate_module(
            read_object(args.input),
            max_start_error_us=args.max_start_error_us,
        )
    except ValueError as exc:
        report = {
            "schema": MODULE_SCHEMA,
            "verdict": "FAIL",
            "physical_complete": False,
            "failure": str(exc),
        }
        emit(report, args.output)
        return 1
    emit(report, args.output)
    return 0


def command_pair(args: argparse.Namespace) -> int:
    try:
        report = validate_pair(
            read_object(args.left),
            read_object(args.right),
            max_start_error_us=args.max_start_error_us,
            max_pair_skew_us=args.max_pair_skew_us,
        )
    except ValueError as exc:
        report = {
            "schema": PAIR_SCHEMA,
            "verdict": "FAIL",
            "physical_complete": False,
            "failure": str(exc),
            "limits": {
                "max_start_error_us": args.max_start_error_us,
                "max_pair_skew_us": args.max_pair_skew_us,
            },
        }
        emit(report, args.output)
        return 1
    emit(report, args.output)
    return 0


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    sub = root.add_subparsers(dest="command", required=True)

    one = sub.add_parser("module", help="validate one physical module trial")
    one.add_argument("--input", type=Path, required=True)
    one.add_argument("--max-start-error-us", type=int, default=1000)
    one.add_argument("--output", type=Path)
    one.set_defaults(func=command_module)

    pair = sub.add_parser("pair", help="validate one synchronized physical pair trial")
    pair.add_argument("--left", type=Path, required=True)
    pair.add_argument("--right", type=Path, required=True)
    pair.add_argument("--max-start-error-us", type=int, default=1000)
    pair.add_argument("--max-pair-skew-us", type=int, default=1000)
    pair.add_argument("--output", type=Path)
    pair.set_defaults(func=command_pair)

    test = sub.add_parser("self-test")
    test.set_defaults(func=lambda _args: self_test())
    return root


def main() -> int:
    args = parser().parse_args()
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
