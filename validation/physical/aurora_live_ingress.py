#!/usr/bin/env python3
"""Stateful, bounded live IEC61937 ingress classifier for Aurora.

This is deliberately separate from aurora_physical_ingress.py. The existing
physical gate remains the strict fixed-capture/SHA lane; this module accepts an
open-ended byte stream, re-locks after discontinuities, and reports every state
transition through bounded diagnostic retention plus exact aggregate counters.
It never claims protected-service compatibility.
"""
from __future__ import annotations

import argparse
import json
import struct
import time
from collections import deque
from dataclasses import asdict, dataclass
from pathlib import Path

SYNC = bytes.fromhex("72f81f4e")
PA, PB = 0xF872, 0x4E1F
TYPE_AC3, TYPE_EAC3 = 0x01, 0x15
PERIOD_BYTES = {TYPE_AC3: 6144, TYPE_EAC3: 24576}
TYPE_NAME = {TYPE_AC3: "AC3", TYPE_EAC3: "EAC3_JOC_CANDIDATE"}
LOCK_STATE = {TYPE_AC3: "LOCKED_AC3", TYPE_EAC3: "LOCKED_JOC"}
SEARCHING, UNCLASSIFIED = "SEARCHING", "UNCLASSIFIED"
DEFAULT_RECORD_LIMIT = 10_000


class LiveIngressError(RuntimeError):
    pass


@dataclass(frozen=True)
class VerdictPolicy:
    """Thresholds that turn observations into a deterministic pass/fail verdict."""

    max_invalid_bursts: int = 0
    max_unclassified_bytes: int | None = None
    max_relocks: int | None = None
    minimum_valid_bursts: int = 1
    require_locked_end: bool = False

    def validate(self) -> None:
        for value in (
            self.max_invalid_bursts,
            self.max_unclassified_bytes,
            self.max_relocks,
            self.minimum_valid_bursts,
        ):
            if value is not None and value < 0:
                raise ValueError("verdict thresholds must be non-negative")


@dataclass
class Transition:
    offset: int
    old_state: str
    new_state: str
    reason: str


@dataclass
class Burst:
    offset: int
    data_type: int
    type_name: str
    period_bytes: int
    payload_bytes: int
    error_flag: bool
    payload_syncword_ok: bool
    padding_zero: bool
    valid: bool
    failure: str | None


def swap16(data: bytes) -> bytes:
    if len(data) % 2:
        raise LiveIngressError("payload length is not 16-bit aligned")
    out = bytearray(len(data))
    out[0::2], out[1::2] = data[1::2], data[0::2]
    return bytes(out)


class LiveIngressClassifier:
    def __init__(
        self,
        *,
        lock_bursts: int = 2,
        unclassified_bytes: int = 6144,
        record_limit: int = DEFAULT_RECORD_LIMIT,
    ):
        if lock_bursts < 1 or unclassified_bytes < 4 or record_limit < 1:
            raise ValueError("invalid classifier thresholds")
        self.lock_bursts = lock_bursts
        self.unclassified_bytes = unclassified_bytes
        self.record_limit = record_limit
        self.buffer = bytearray()
        self.stream_offset = 0
        self.state = SEARCHING
        self.candidate_type: int | None = None
        self.candidate_count = 0

        # Diagnostic samples are bounded. Exact totals are kept separately so a 24 h soak does
        # not trade correctness for memory safety.
        self.transitions: deque[Transition] = deque(maxlen=record_limit)
        self.bursts: deque[Burst] = deque(maxlen=record_limit)
        self.gaps: deque[dict] = deque(maxlen=record_limit)
        self.transition_total = 0
        self.burst_total = 0
        self.valid_burst_total = 0
        self.gap_total = 0

        self.relock_count = 0
        self.invalid_bursts = 0
        self.unclassified_total = 0
        self.type_counts = {"AC3": 0, "EAC3_JOC_CANDIDATE": 0}

    def transition(self, new_state: str, reason: str) -> None:
        if new_state == self.state:
            return
        self.transition_total += 1
        self.transitions.append(Transition(self.stream_offset, self.state, new_state, reason))
        self.state = new_state

    def _mark_gap(self, data: bytes, reason: str) -> None:
        if not data:
            return
        nonzero = any(data)
        self.gap_total += 1
        self.gaps.append(
            {
                "offset": self.stream_offset,
                "bytes": len(data),
                "nonzero": nonzero,
                "reason": reason,
            }
        )
        self.unclassified_total += len(data)
        if self.state.startswith("LOCKED_"):
            self.relock_count += 1
        if len(data) >= self.unclassified_bytes or nonzero:
            self.transition(UNCLASSIFIED, reason)
        else:
            self.transition(SEARCHING, reason)
        self.candidate_type = None
        self.candidate_count = 0
        self.stream_offset += len(data)

    @staticmethod
    def _payload_bytes(data_type: int, pd: int) -> int:
        if data_type == TYPE_EAC3:
            return pd
        if data_type == TYPE_AC3:
            return (pd + 7) // 8
        return 0

    def _parse_burst(self, burst: bytes, offset: int) -> Burst:
        pa, pb, pc, pd = struct.unpack_from("<HHHH", burst, 0)
        data_type = pc & 0x7F
        error_flag = bool(pc & 0x80)
        period = PERIOD_BYTES.get(data_type, len(burst))
        payload_bytes = self._payload_bytes(data_type, pd)
        failure = None
        sync_ok = False
        padding_zero = False
        if (pa, pb) != (PA, PB):
            failure = "invalid-preamble"
        elif data_type not in PERIOD_BYTES:
            failure = f"unsupported-data-type-0x{data_type:02x}"
        elif error_flag:
            failure = "pc-error-flag"
        elif payload_bytes <= 0 or 8 + payload_bytes > period:
            failure = "invalid-payload-length"
        elif payload_bytes % 2:
            failure = "odd-payload-length"
        else:
            raw = swap16(burst[8 : 8 + payload_bytes])
            sync_ok = raw.startswith(b"\x0b\x77")
            padding_zero = not any(burst[8 + payload_bytes : period])
            if not sync_ok:
                failure = "payload-syncword"
            elif not padding_zero:
                failure = "nonzero-padding"
        return Burst(
            offset=offset,
            data_type=data_type,
            type_name=TYPE_NAME.get(data_type, f"TYPE_0x{data_type:02x}"),
            period_bytes=period,
            payload_bytes=payload_bytes,
            error_flag=error_flag,
            payload_syncword_ok=sync_ok,
            padding_zero=padding_zero,
            valid=failure is None,
            failure=failure,
        )

    def _accept_valid_burst(self, item: Burst) -> None:
        self.valid_burst_total += 1
        self.type_counts[item.type_name] = self.type_counts.get(item.type_name, 0) + 1
        if self.candidate_type != item.data_type:
            if self.state.startswith("LOCKED_"):
                self.relock_count += 1
                self.transition(SEARCHING, "format-change")
            self.candidate_type = item.data_type
            self.candidate_count = 1
        else:
            self.candidate_count += 1
        if self.candidate_count >= self.lock_bursts:
            self.transition(LOCK_STATE[item.data_type], f"stable-{item.type_name.lower()}")

    def feed(self, data: bytes, *, final: bool = False) -> None:
        self.buffer.extend(data)
        while True:
            sync = self.buffer.find(SYNC)
            if sync < 0:
                keep = 0 if final else min(3, len(self.buffer))
                emit = len(self.buffer) - keep
                if emit > 0:
                    gap = bytes(self.buffer[:emit])
                    del self.buffer[:emit]
                    self._mark_gap(gap, "no-iec-sync")
                break
            if sync > 0:
                gap = bytes(self.buffer[:sync])
                del self.buffer[:sync]
                self._mark_gap(gap, "relock-after-gap-or-byte-slip")
                continue
            if len(self.buffer) < 8:
                break
            _, _, pc, _ = struct.unpack_from("<HHHH", self.buffer, 0)
            data_type = pc & 0x7F
            period = PERIOD_BYTES.get(data_type)
            if period is None:
                self.invalid_bursts += 1
                self.transition(SEARCHING, f"unsupported-data-type-0x{data_type:02x}")
                del self.buffer[:4]
                self.stream_offset += 4
                continue
            if len(self.buffer) < period:
                break
            raw_burst = bytes(self.buffer[:period])
            item = self._parse_burst(raw_burst, self.stream_offset)
            self.burst_total += 1
            self.bursts.append(item)
            if item.valid:
                self._accept_valid_burst(item)
            else:
                self.invalid_bursts += 1
                if self.state.startswith("LOCKED_"):
                    self.relock_count += 1
                self.transition(SEARCHING, item.failure or "invalid-burst")
                self.candidate_type = None
                self.candidate_count = 0
            del self.buffer[:period]
            self.stream_offset += period
        if final and self.buffer:
            tail = bytes(self.buffer)
            self.buffer.clear()
            self._mark_gap(tail, "trailing-unclassified")

    def _verdict_failures(self, policy: VerdictPolicy) -> list[str]:
        policy.validate()
        failures: list[str] = []
        if self.invalid_bursts > policy.max_invalid_bursts:
            failures.append(
                f"invalid_bursts={self.invalid_bursts} exceeds {policy.max_invalid_bursts}"
            )
        if (
            policy.max_unclassified_bytes is not None
            and self.unclassified_total > policy.max_unclassified_bytes
        ):
            failures.append(
                f"unclassified_bytes={self.unclassified_total} exceeds "
                f"{policy.max_unclassified_bytes}"
            )
        if policy.max_relocks is not None and self.relock_count > policy.max_relocks:
            failures.append(f"relocks={self.relock_count} exceeds {policy.max_relocks}")
        if self.valid_burst_total < policy.minimum_valid_bursts:
            failures.append(
                f"valid_bursts={self.valid_burst_total} below {policy.minimum_valid_bursts}"
            )
        if policy.require_locked_end and not self.state.startswith("LOCKED_"):
            failures.append(f"final_state={self.state} is not locked")
        return failures

    def report(self, policy: VerdictPolicy | None = None) -> dict:
        policy = policy or VerdictPolicy()
        failures = self._verdict_failures(policy)
        return {
            "schema_version": 1,
            "verdict": "fail" if failures else "pass",
            "verdict_failures": failures,
            "state": self.state,
            "stream_bytes_consumed": self.stream_offset,
            "burst_count": self.burst_total,
            "valid_bursts": self.valid_burst_total,
            "invalid_bursts": self.invalid_bursts,
            "relock_count": self.relock_count,
            "unclassified_bytes": self.unclassified_total,
            "type_counts": self.type_counts,
            "transition_count": self.transition_total,
            "gap_count": self.gap_total,
            "retention": {
                "record_limit": self.record_limit,
                "retained_transitions": len(self.transitions),
                "retained_bursts": len(self.bursts),
                "retained_gaps": len(self.gaps),
                "dropped_transition_records": max(0, self.transition_total - len(self.transitions)),
                "dropped_burst_records": max(0, self.burst_total - len(self.bursts)),
                "dropped_gap_records": max(0, self.gap_total - len(self.gaps)),
            },
            "policy": asdict(policy),
            "transitions": [asdict(item) for item in self.transitions],
            "gaps": list(self.gaps),
            "bursts": [asdict(item) for item in self.bursts],
            "truth_boundary": (
                "Stateful bounded IEC61937 live-ingress classification/relock evidence only. "
                "No physical eARC link, protected-service compatibility, decoder correctness, "
                "or physical multichannel output is proven by this classifier."
            ),
        }


def build_burst(data_type: int, marker: int) -> bytes:
    period = PERIOD_BYTES[data_type]
    payload = b"\x0b\x77" + bytes([marker & 0xFF]) * 1022
    framed = swap16(payload)
    pd = len(payload) if data_type == TYPE_EAC3 else len(payload) * 8
    preamble = struct.pack("<HHHH", PA, PB, data_type, pd)
    return preamble + framed + bytes(period - len(preamble) - len(framed))


def feed_chunked(classifier: LiveIngressClassifier, data: bytes) -> None:
    sizes = (1, 7, 4093, 29, 8192, 3, 997, 16384, 61)
    offset = 0
    index = 0
    while offset < len(data):
        size = sizes[index % len(sizes)]
        classifier.feed(data[offset : offset + size])
        offset += size
        index += 1
    classifier.feed(b"", final=True)


def self_test() -> int:
    classifier = LiveIngressClassifier(lock_bursts=2, unclassified_bytes=4096)
    pcm = bytes((index % 251) + 1 for index in range(5000))
    stream = bytearray(pcm)
    stream += build_burst(TYPE_EAC3, 1)
    stream += build_burst(TYPE_EAC3, 2)
    stream += bytes(37)
    stream += build_burst(TYPE_EAC3, 3)
    stream += build_burst(TYPE_EAC3, 4)
    stream += b"\x99"
    stream += build_burst(TYPE_EAC3, 5)
    stream += build_burst(TYPE_EAC3, 6)
    stream += build_burst(TYPE_AC3, 7)
    stream += build_burst(TYPE_AC3, 8)
    stream += pcm
    stream += build_burst(TYPE_EAC3, 9)
    stream += build_burst(TYPE_EAC3, 10)
    feed_chunked(classifier, bytes(stream))
    report = classifier.report()
    states = [item["new_state"] for item in report["transitions"]]
    if report["verdict"] != "pass":
        raise AssertionError(report)
    if "LOCKED_JOC" not in states or "LOCKED_AC3" not in states:
        raise AssertionError(f"format locks missing: {states}")
    if "UNCLASSIFIED" not in states:
        raise AssertionError("sync-free PCM/unclassified state was not exposed")
    if report["type_counts"]["EAC3_JOC_CANDIDATE"] != 8:
        raise AssertionError(report)
    if report["type_counts"]["AC3"] != 2:
        raise AssertionError(report)
    if report["relock_count"] < 3:
        raise AssertionError(f"expected multiple explicit relocks, got {report['relock_count']}")

    corrupt = bytearray(build_burst(TYPE_EAC3, 9))
    corrupt[-1] = 1
    bad = LiveIngressClassifier(lock_bursts=1)
    bad.feed(bytes(corrupt), final=True)
    bad_report = bad.report()
    if bad.invalid_bursts != 1 or bad.state.startswith("LOCKED_"):
        raise AssertionError("nonzero padding did not fail closed")
    if bad_report["verdict"] != "fail":
        raise AssertionError("malformed burst produced a false PASS verdict")

    bounded = LiveIngressClassifier(lock_bursts=1, record_limit=8)
    for marker in range(32):
        bounded.feed(build_burst(TYPE_EAC3, marker))
    bounded.feed(b"", final=True)
    bounded_report = bounded.report()
    if bounded_report["burst_count"] != 32 or bounded_report["valid_bursts"] != 32:
        raise AssertionError("bounded retention changed exact aggregate counters")
    if len(bounded_report["bursts"]) != 8:
        raise AssertionError("burst diagnostic retention is not bounded")
    if bounded_report["retention"]["dropped_burst_records"] != 24:
        raise AssertionError("bounded retention drop accounting is wrong")

    strict = classifier.report(VerdictPolicy(max_relocks=0))
    if strict["verdict"] != "fail" or not strict["verdict_failures"]:
        raise AssertionError("threshold policy did not produce deterministic FAIL")

    print(
        "AURORA-LIVE-INGRESS-SELFTEST-PASS "
        f"bursts={report['valid_bursts']} relocks={report['relock_count']} "
        f"transitions={report['transition_count']} bounded_records=8"
    )
    return 0


def analyze_file(
    path: Path,
    *,
    chunk_bytes: int,
    policy: VerdictPolicy,
    record_limit: int,
) -> dict:
    if chunk_bytes < 1:
        raise LiveIngressError("chunk size must be positive")
    classifier = LiveIngressClassifier(record_limit=record_limit)
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(chunk_bytes), b""):
            classifier.feed(chunk)
    classifier.feed(b"", final=True)
    return classifier.report(policy)


def follow_file(
    path: Path,
    *,
    poll_ms: int,
    idle_timeout: float,
    max_seconds: float | None,
    policy: VerdictPolicy,
    record_limit: int,
) -> dict:
    if poll_ms < 1 or idle_timeout <= 0:
        raise LiveIngressError("invalid live follow timing")
    classifier = LiveIngressClassifier(record_limit=record_limit)
    started = time.monotonic()
    last_data = started
    position = 0
    while True:
        if path.exists():
            with path.open("rb") as handle:
                handle.seek(position)
                chunk = handle.read()
            if chunk:
                classifier.feed(chunk)
                position += len(chunk)
                last_data = time.monotonic()
        now = time.monotonic()
        if max_seconds is not None and now - started >= max_seconds:
            break
        if now - last_data >= idle_timeout:
            break
        time.sleep(poll_ms / 1000.0)
    classifier.feed(b"", final=True)
    report = classifier.report(policy)
    report["follow"] = {
        "path": str(path),
        "bytes_read": position,
        "wall_seconds": time.monotonic() - started,
        "idle_timeout_seconds": idle_timeout,
    }
    return report


def write_report(report: dict, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def add_policy_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--max-invalid-bursts", type=int, default=0)
    parser.add_argument("--max-unclassified-bytes", type=int)
    parser.add_argument("--max-relocks", type=int)
    parser.add_argument("--minimum-valid-bursts", type=int, default=1)
    parser.add_argument("--require-locked-end", action="store_true")
    parser.add_argument("--record-limit", type=int, default=DEFAULT_RECORD_LIMIT)


def policy_from_args(args: argparse.Namespace) -> VerdictPolicy:
    policy = VerdictPolicy(
        max_invalid_bursts=args.max_invalid_bursts,
        max_unclassified_bytes=args.max_unclassified_bytes,
        max_relocks=args.max_relocks,
        minimum_valid_bursts=args.minimum_valid_bursts,
        require_locked_end=args.require_locked_end,
    )
    policy.validate()
    if args.record_limit < 1:
        raise LiveIngressError("record limit must be positive")
    return policy


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("self-test", help="exercise live joins, thresholds and bounded retention")

    analyze = sub.add_parser("analyze", help="classify a finite capture using the live state machine")
    analyze.add_argument("--input", type=Path, required=True)
    analyze.add_argument("--report", type=Path, required=True)
    analyze.add_argument("--chunk-bytes", type=int, default=4096)
    add_policy_arguments(analyze)

    follow = sub.add_parser("follow", help="follow a growing canonical IEC61937 capture file")
    follow.add_argument("--capture", type=Path, required=True)
    follow.add_argument("--report", type=Path, required=True)
    follow.add_argument("--poll-ms", type=int, default=100)
    follow.add_argument("--idle-timeout", type=float, default=5.0)
    follow.add_argument("--max-seconds", type=float)
    add_policy_arguments(follow)
    args = parser.parse_args()

    if args.command == "self-test":
        return self_test()

    policy = policy_from_args(args)
    if args.command == "analyze":
        if not args.input.is_file():
            raise LiveIngressError(f"input does not exist: {args.input}")
        report = analyze_file(
            args.input,
            chunk_bytes=args.chunk_bytes,
            policy=policy,
            record_limit=args.record_limit,
        )
    else:
        report = follow_file(
            args.capture,
            poll_ms=args.poll_ms,
            idle_timeout=args.idle_timeout,
            max_seconds=args.max_seconds,
            policy=policy,
            record_limit=args.record_limit,
        )
    write_report(report, args.report)
    return 0 if report["verdict"] == "pass" else 2


if __name__ == "__main__":
    raise SystemExit(main())
