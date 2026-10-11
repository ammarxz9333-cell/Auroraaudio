#!/usr/bin/env python3
"""Fail-closed physical acceptance for Aurora detachable speaker modules.

The tool validates already-built physical module evidence, synchronized pairs,
and can correlate raw ESP listener/scheduler snapshots with an independent
logic-analyzer/electrical capture. A PASS is only possible when actual measured
output timing, PTP lock, stream progress, mute safety, and shared clock identity
satisfy the configured limits.

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
LISTENER_SCHEMA = "aurora.esp-avb.listener-snapshot.v1"
ENDPOINT_SCHEMA = "aurora.esp-avb.endpoint-control-snapshot.v1"
ELECTRICAL_SCHEMA = "aurora.detachable-electrical-capture.v1"


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


def _validate_listener_snapshot(raw: dict[str, Any], label: str) -> dict[str, Any]:
    if raw.get("schema") != LISTENER_SCHEMA:
        raise ValueError(f"{label} schema must be {LISTENER_SCHEMA}")
    if raw.get("verdict") != "PASS":
        raise ValueError(f"{label} listener snapshot verdict must be PASS")

    result = {
        "epoch_id": req_str(raw, "epoch_id"),
        "stream_id": req_str(raw, "stream_id"),
        "grandmaster_id": req_str(raw, "grandmaster_id"),
        "sample_rate_hz": req_int(raw, "sample_rate_hz"),
        "channels": req_int(raw, "channels"),
        "bit_depth": req_int(raw, "bit_depth"),
        "acmp_connected": req_bool(raw, "acmp_connected"),
        "gptp_locked": req_bool(raw, "gptp_locked"),
        "rx_counter": req_int(raw, "rx_counter"),
        "capture_unix_ms": req_int(raw, "capture_unix_ms"),
    }
    if not result["acmp_connected"] or not result["gptp_locked"]:
        raise ValueError(f"{label} listener must be connected and gPTP-locked")
    if (
        result["sample_rate_hz"],
        result["channels"],
        result["bit_depth"],
    ) != (48_000, 2, 24):
        raise ValueError(f"{label} listener media contract must be 48000 Hz / 2 channels / 24 bit")
    if result["rx_counter"] < 0 or result["capture_unix_ms"] <= 0:
        raise ValueError(f"{label} listener counters/timestamps are invalid")
    return result


def correlate_module_trial(
    before_raw: dict[str, Any],
    after_raw: dict[str, Any],
    endpoint_raw: dict[str, Any],
    electrical_raw: dict[str, Any],
    *,
    max_start_error_us: int,
    max_scheduler_late_us: int,
) -> dict[str, Any]:
    if max_scheduler_late_us < 0:
        raise ValueError("max_scheduler_late_us must be non-negative")

    before = _validate_listener_snapshot(before_raw, "before")
    after = _validate_listener_snapshot(after_raw, "after")

    for key in (
        "epoch_id",
        "stream_id",
        "grandmaster_id",
        "sample_rate_hz",
        "channels",
        "bit_depth",
    ):
        if before[key] != after[key]:
            raise ValueError(f"listener before/after {key} mismatch")
    if after["capture_unix_ms"] <= before["capture_unix_ms"]:
        raise ValueError("listener after capture must be later than before capture")
    if after["rx_counter"] <= before["rx_counter"]:
        raise ValueError("listener receive counter did not advance")

    if endpoint_raw.get("schema") != ENDPOINT_SCHEMA:
        raise ValueError(f"endpoint schema must be {ENDPOINT_SCHEMA}")
    if endpoint_raw.get("verdict") != "PASS":
        raise ValueError("endpoint scheduler snapshot verdict must be PASS")
    if req_str(endpoint_raw, "epoch_id") != before["epoch_id"]:
        raise ValueError("endpoint/listener epoch_id mismatch")
    if not req_bool(endpoint_raw, "gptp_locked"):
        raise ValueError("endpoint scheduler snapshot is not gPTP-locked")
    if req_str(endpoint_raw, "grandmaster_id") != before["grandmaster_id"]:
        raise ValueError("endpoint/listener grandmaster mismatch")

    requested = req_int(endpoint_raw, "requested_start_ptp_ns")
    scheduler = req_int(endpoint_raw, "scheduler_unmute_ptp_ns")
    unmute_count = req_int(endpoint_raw, "scheduler_unmute_count")
    deadline_fault_count = req_int(endpoint_raw, "deadline_fault_count")
    phase = req_int(endpoint_raw, "phase")
    muted = req_bool(endpoint_raw, "muted")
    active_role0 = req_int(endpoint_raw, "active_role0")
    active_role1 = req_int(endpoint_raw, "active_role1")

    if requested <= 0 or scheduler <= 0:
        raise ValueError("endpoint scheduler has no completed activation timing")
    if unmute_count <= 0:
        raise ValueError("endpoint scheduler has no successful unmute event")
    if deadline_fault_count != 0:
        raise ValueError("endpoint scheduler reports deadline faults")
    if phase != 4 or muted:
        raise ValueError("endpoint snapshot must be committed and logically unmuted")
    if not 0 <= active_role0 <= 11:
        raise ValueError("endpoint active_role0 is outside canonical 7.1.4 roles")
    if active_role1 != 0xFF and not 0 <= active_role1 <= 11:
        raise ValueError("endpoint active_role1 is outside canonical 7.1.4 roles")

    scheduler_late_ns = scheduler - requested
    if scheduler_late_ns < 0:
        raise ValueError("firmware scheduler requested unmute before the PTP deadline")
    if scheduler_late_ns > max_scheduler_late_us * 1000:
        raise ValueError(
            f"firmware scheduler lateness {scheduler_late_ns / 1000.0:.3f} us exceeds "
            f"{max_scheduler_late_us} us"
        )

    if electrical_raw.get("schema") != ELECTRICAL_SCHEMA:
        raise ValueError(f"electrical schema must be {ELECTRICAL_SCHEMA}")
    module_id = req_str(electrical_raw, "module_id")
    capture_id = req_str(electrical_raw, "capture_id")
    measurement_method = req_str(electrical_raw, "measurement_method")

    raw_module = {
        "schema": MODULE_SCHEMA,
        "module_id": module_id,
        "epoch_id": before["epoch_id"],
        "stream_id": before["stream_id"],
        "grandmaster_id": before["grandmaster_id"],
        "sample_rate_hz": before["sample_rate_hz"],
        "channels": before["channels"],
        "bit_depth": before["bit_depth"],
        "gptp_locked": True,
        "acmp_connected": True,
        "rx_counter_before": before["rx_counter"],
        "rx_counter_after": after["rx_counter"],
        "requested_start_ptp_ns": requested,
        "measured_start_ptp_ns": req_int(electrical_raw, "measured_start_ptp_ns"),
        "pre_roll_samples": req_int(electrical_raw, "pre_roll_samples"),
        "underflows": req_int(electrical_raw, "underflows"),
        "reconnects": req_int(electrical_raw, "reconnects"),
        "hardware_muted_before_start": req_bool(
            electrical_raw, "hardware_muted_before_start"
        ),
        "hardware_mute_on_stop": req_bool(electrical_raw, "hardware_mute_on_stop"),
        "hardware_mute_on_sync_loss": req_bool(
            electrical_raw, "hardware_mute_on_sync_loss"
        ),
    }

    report = validate_module(raw_module, max_start_error_us=max_start_error_us)
    report["correlation"] = {
        "capture_id": capture_id,
        "measurement_method": measurement_method,
        "listener_before_unix_ms": before["capture_unix_ms"],
        "listener_after_unix_ms": after["capture_unix_ms"],
        "firmware_scheduler_unmute_ptp_ns": scheduler,
        "firmware_scheduler_late_ns": scheduler_late_ns,
        "firmware_scheduler_late_us": scheduler_late_ns / 1000.0,
        "scheduler_unmute_count": unmute_count,
        "deadline_fault_count": deadline_fault_count,
        "active_roles": [active_role0, active_role1],
        "truth_boundary": (
            "firmware scheduler timing is correlated with, but does not replace, "
            "the independent electrical measured_start_ptp_ns"
        ),
    }
    return report


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


def listener_fixture(rx: int, capture_ms: int) -> dict[str, Any]:
    return {
        "schema": LISTENER_SCHEMA,
        "verdict": "PASS",
        "epoch_id": "epoch-42",
        "capture_unix_ms": capture_ms,
        "stream_id": "02:00:00:00:00:01:00:0a",
        "sample_rate_hz": 48_000,
        "channels": 2,
        "bit_depth": 24,
        "acmp_connected": True,
        "gptp_locked": True,
        "grandmaster_id": "02:00:00:ff:fe:00:00:01",
        "rx_counter": rx,
        "last_rx_us": capture_ms * 1000 - 100,
    }


def endpoint_fixture() -> dict[str, Any]:
    return {
        "schema": ENDPOINT_SCHEMA,
        "verdict": "PASS",
        "epoch_id": "epoch-42",
        "capture_unix_ms": 6_000,
        "gptp_locked": True,
        "grandmaster_id": "02:00:00:ff:fe:00:00:01",
        "active_epoch": 42,
        "active_role0": 4,
        "active_role1": 10,
        "requested_start_ptp_ns": 2_000_000_000,
        "scheduler_unmute_ptp_ns": 2_000_090_000,
        "scheduler_unmute_count": 1,
        "deadline_fault_count": 0,
        "phase": 4,
        "muted": False,
    }


def electrical_fixture() -> dict[str, Any]:
    return {
        "schema": ELECTRICAL_SCHEMA,
        "module_id": "left-pod",
        "capture_id": "logic-001",
        "measurement_method": "logic-analyzer-output-gate+audio-edge",
        "measured_start_ptp_ns": 2_000_120_000,
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

    before = listener_fixture(10_000, 1_000)
    after = listener_fixture(30_000, 7_000)
    endpoint = endpoint_fixture()
    electrical = electrical_fixture()
    correlated = correlate_module_trial(
        before,
        after,
        endpoint,
        electrical,
        max_start_error_us=1000,
        max_scheduler_late_us=2000,
    )
    assert correlated["verdict"] == "PASS"
    assert correlated["correlation"]["firmware_scheduler_late_ns"] == 90_000
    assert correlated["measured_start_ptp_ns"] == 2_000_120_000

    correlation_negative = {
        "listener-gm-change": lambda b, a, e, x: a.__setitem__(
            "grandmaster_id", "02:00:00:ff:fe:00:00:02"
        ),
        "listener-no-progress": lambda b, a, e, x: a.__setitem__(
            "rx_counter", b["rx_counter"]
        ),
        "endpoint-epoch": lambda b, a, e, x: e.__setitem__("epoch_id", "epoch-43"),
        "endpoint-deadline-fault": lambda b, a, e, x: e.__setitem__(
            "deadline_fault_count", 1
        ),
        "scheduler-early": lambda b, a, e, x: e.__setitem__(
            "scheduler_unmute_ptp_ns", 1_999_999_000
        ),
        "scheduler-late": lambda b, a, e, x: e.__setitem__(
            "scheduler_unmute_ptp_ns", 2_003_000_000
        ),
        "endpoint-muted": lambda b, a, e, x: e.__setitem__("muted", True),
        "electrical-mute-missing": lambda b, a, e, x: x.__setitem__(
            "hardware_mute_on_sync_loss", False
        ),
    }
    for name, mutate in correlation_negative.items():
        b = listener_fixture(10_000, 1_000)
        a = listener_fixture(30_000, 7_000)
        e = endpoint_fixture()
        x = electrical_fixture()
        mutate(b, a, e, x)
        try:
            correlate_module_trial(
                b,
                a,
                e,
                x,
                max_start_error_us=1000,
                max_scheduler_late_us=2000,
            )
        except ValueError:
            pass
        else:
            raise AssertionError(f"correlation negative unexpectedly passed: {name}")

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
        "positive=1 negative=12 physical-pass=synthetic-only "
        "correlation-positive=1 correlation-negative=8"
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


def command_correlate(args: argparse.Namespace) -> int:
    try:
        report = correlate_module_trial(
            read_object(args.before_listener),
            read_object(args.after_listener),
            read_object(args.endpoint),
            read_object(args.electrical),
            max_start_error_us=args.max_start_error_us,
            max_scheduler_late_us=args.max_scheduler_late_us,
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

    correlate = sub.add_parser(
        "correlate",
        help="build one module trial from listener, scheduler, and electrical captures",
    )
    correlate.add_argument("--before-listener", type=Path, required=True)
    correlate.add_argument("--after-listener", type=Path, required=True)
    correlate.add_argument("--endpoint", type=Path, required=True)
    correlate.add_argument("--electrical", type=Path, required=True)
    correlate.add_argument("--max-start-error-us", type=int, default=1000)
    correlate.add_argument("--max-scheduler-late-us", type=int, default=2000)
    correlate.add_argument("--output", type=Path)
    correlate.set_defaults(func=command_correlate)

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
