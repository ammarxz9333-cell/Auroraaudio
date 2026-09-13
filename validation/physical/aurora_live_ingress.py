#!/usr/bin/env python3
"""Stateful live IEC61937 ingress classifier for Aurora.

This is deliberately separate from aurora_physical_ingress.py. The existing
physical gate remains the strict fixed-capture/SHA lane; this module accepts an
open-ended byte stream, re-locks after discontinuities, and reports every state
transition explicitly. It never claims protected-service compatibility.
"""
from __future__ import annotations

import argparse
import json
import struct
import time
from dataclasses import asdict, dataclass
from pathlib import Path

SYNC = bytes.fromhex("72f81f4e")
PA, PB = 0xF872, 0x4E1F
TYPE_AC3, TYPE_EAC3 = 0x01, 0x15
PERIOD_BYTES = {TYPE_AC3: 6144, TYPE_EAC3: 24576}
TYPE_NAME = {TYPE_AC3: "AC3", TYPE_EAC3: "EAC3_JOC_CANDIDATE"}
LOCK_STATE = {TYPE_AC3: "LOCKED_AC3", TYPE_EAC3: "LOCKED_JOC"}
SEARCHING, UNCLASSIFIED = "SEARCHING", "UNCLASSIFIED"


class LiveIngressError(RuntimeError):
    pass
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
    def __init__(self, *, lock_bursts: int = 2, unclassified_bytes: int = 6144):
        if lock_bursts < 1 or unclassified_bytes < 4:
            raise ValueError("invalid classifier thresholds")
        self.lock_bursts = lock_bursts
        self.unclassified_bytes = unclassified_bytes
        self.buffer = bytearray()
        self.stream_offset = 0
        self.state = SEARCHING
        self.candidate_type: int | None = None
        self.candidate_count = 0
        self.transitions: list[Transition] = []
        self.bursts: list[Burst] = []
        self.gaps: list[dict] = []
        self.relock_count = 0
        self.invalid_bursts = 0
        self.unclassified_total = 0
        self.type_counts = {"AC3": 0, "EAC3_JOC_CANDIDATE": 0}

    def transition(self, new_state: str, reason: str) -> None:
        if new_state == self.state:
            return
        self.transitions.append(Transition(self.stream_offset, self.state, new_state, reason))
        self.state = new_state
    def _mark_gap(self, data: bytes, reason: str) -> None:
        if not data:
            return
        nonzero = any(data)
        self.gaps.append({
            "offset": self.stream_offset,
            "bytes": len(data),
            "nonzero": nonzero,
            "reason": reason,
        })
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
            raw = swap16(burst[8:8 + payload_bytes])
            sync_ok = raw.startswith(b"\x0b\x77")
            padding_zero = not any(burst[8 + payload_bytes:period])
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

    def report(self) -> dict:
        return {
            "schema_version": 1,
            "verdict": "pass",
            "state": self.state,
            "stream_bytes_consumed": self.stream_offset,
            "burst_count": len(self.bursts),
            "valid_bursts": sum(1 for item in self.bursts if item.valid),
            "invalid_bursts": self.invalid_bursts,
            "relock_count": self.relock_count,
            "unclassified_bytes": self.unclassified_total,
            "type_counts": self.type_counts,
            "transitions": [asdict(item) for item in self.transitions],
            "gaps": self.gaps,
            "bursts": [asdict(item) for item in self.bursts],
            "truth_boundary": (
                "Stateful IEC61937 live-ingress classification/relock evidence only. "
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
        classifier.feed(data[offset:offset + size])
        offset += size
        index += 1
    classifier.feed(b"", final=True)


def self_test() -> int:
    classifier = LiveIngressClassifier(lock_bursts=2, unclassified_bytes=4096)
    pcm = bytes((index % 251) + 1 for index in range(5000))
    stream = bytearray(pcm)
    stream += build_burst(TYPE_EAC3, 1)
    stream += build_burst(TYPE_EAC3, 2)
    stream += bytes(37)  # explicit discontinuity after a locked JOC train
    stream += build_burst(TYPE_EAC3, 3)
    stream += build_burst(TYPE_EAC3, 4)  # regain LOCKED_JOC
    stream += b"\x99"  # one-byte transport slip while locked
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
    if report["invalid_bursts"]:
        raise AssertionError(f"unexpected invalid bursts: {report['invalid_bursts']}")

    corrupt = bytearray(build_burst(TYPE_EAC3, 9))
    corrupt[-1] = 1
    bad = LiveIngressClassifier(lock_bursts=1)
    bad.feed(bytes(corrupt), final=True)
    if bad.invalid_bursts != 1 or bad.state.startswith("LOCKED_"):
        raise AssertionError("nonzero padding did not fail closed")
    print(
        "AURORA-LIVE-INGRESS-SELFTEST-PASS "
        f"bursts={report['valid_bursts']} relocks={report['relock_count']} transitions={len(states)}"
    )
    return 0
def analyze_file(path: Path, *, chunk_bytes: int) -> dict:
    if chunk_bytes < 1:
        raise LiveIngressError("chunk size must be positive")
    classifier = LiveIngressClassifier()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(chunk_bytes), b""):
            classifier.feed(chunk)
    classifier.feed(b"", final=True)
    return classifier.report()


def follow_file(path: Path, *, poll_ms: int, idle_timeout: float, max_seconds: float | None) -> dict:
    if poll_ms < 1 or idle_timeout <= 0:
        raise LiveIngressError("invalid live follow timing")
    classifier = LiveIngressClassifier()
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
    report = classifier.report()
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("self-test", help="exercise live joins, format changes, gaps and byte slips")
    analyze = sub.add_parser("analyze", help="classify a finite capture using the live state machine")
    analyze.add_argument("--input", type=Path, required=True)
    analyze.add_argument("--report", type=Path, required=True)
    analyze.add_argument("--chunk-bytes", type=int, default=4096)

    follow = sub.add_parser("follow", help="follow a growing canonical IEC61937 capture file")
    follow.add_argument("--capture", type=Path, required=True)
    follow.add_argument("--report", type=Path, required=True)
    follow.add_argument("--poll-ms", type=int, default=100)
    follow.add_argument("--idle-timeout", type=float, default=5.0)
    follow.add_argument("--max-seconds", type=float)
    args = parser.parse_args()

    if args.command == "self-test":
        return self_test()
    if args.command == "analyze":
        if not args.input.is_file():
            raise LiveIngressError(f"input does not exist: {args.input}")
        report = analyze_file(args.input, chunk_bytes=args.chunk_bytes)
        write_report(report, args.report)
    else:
        report = follow_file(args.capture, poll_ms=args.poll_ms,
                             idle_timeout=args.idle_timeout, max_seconds=args.max_seconds)
        write_report(report, args.report)
    print(
        "AURORA-LIVE-INGRESS-PASS "
        f"state={report['state']} bursts={report['valid_bursts']} "
        f"invalid={report['invalid_bursts']} relocks={report['relock_count']}"
    )
    print(f"report={args.report}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except LiveIngressError as exc:
        print(f"AURORA-LIVE-INGRESS-FAIL: {exc}")
        raise SystemExit(1)
