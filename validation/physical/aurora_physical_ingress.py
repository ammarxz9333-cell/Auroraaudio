#!/usr/bin/env python3
"""Validate a physical IEC61937 E-AC-3 capture for Aurora issue #143.

The gate is intentionally narrow and fail-closed. It validates a captured physical
IEC61937 byte stream before that capture is admitted to Aurora's already-proven
moving-JOC software path.

For E-AC-3 (IEC61937 data type 0x15), the framing used by Aurora's reference path
has Pa/Pb 0xF872/0x4E1F, Pc data type 0x15, Pd as payload length in bytes, a
24576-byte repetition period, and 16-bit word-swapped payload bytes in the
little-endian IEC byte stream.

A pass here is ingress/capture evidence only. It is not physical 12-channel output,
Dolby conformance/certification, or DRM-service compatibility evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

SYNC = bytes.fromhex("72f81f4e")
PA = 0xF872
PB = 0x4E1F
EAC3_TYPE = 0x15
EAC3_BURST_BYTES = 24576
PINNED_MOVING_SHA256 = "0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0"


class GateFailure(RuntimeError):
    pass


@dataclass
class BurstEvidence:
    index: int
    offset: int
    pc: int
    data_type: int
    error_flag: bool
    payload_bytes: int
    payload_sha256: str
    payload_syncword_ok: bool
    padding_bytes: int
    padding_zero: bool


@dataclass
class GateReport:
    schema_version: int
    verdict: str
    capture_path: str
    capture_bytes: int
    capture_sha256: str
    capture_start_monotonic_ns: int | None
    capture_end_monotonic_ns: int | None
    capture_duration_ns: int | None
    capture_reset_count: int | None
    capture_drop_count: int | None
    prefix_bytes: int
    suffix_bytes: int
    burst_period_bytes: int
    burst_count: int
    expected_burst_count: int | None
    data_type: int
    reconstructed_payload_bytes: int
    reconstructed_payload_sha256: str
    expected_payload_sha256: str | None
    payload_identity_match: bool | None
    zero_prefix: bool
    zero_suffix: bool
    all_padding_zero: bool
    failures: list[str]
    bursts: list[dict]
    truth_boundary: str


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def swap16(data: bytes) -> bytes:
    if len(data) % 2:
        raise GateFailure(f"E-AC-3 payload length must be even for exact 16-bit word swap, got {len(data)}")
    out = bytearray(len(data))
    out[0::2] = data[1::2]
    out[1::2] = data[0::2]
    return bytes(out)


def consecutive_burst_offsets(data: bytes, first: int, period: int) -> list[int]:
    """Return only syncs on the exact fixed-period grid starting at first.

    Do not scan for every sync-looking byte pattern: a compressed payload may contain
    the four sync bytes by coincidence. A physical discontinuity instead manifests as
    a missing sync at an expected grid position and non-zero residual suffix data.
    """
    offsets: list[int] = []
    offset = first
    while offset + 8 <= len(data) and data[offset : offset + 4] == SYNC:
        offsets.append(offset)
        offset += period
    return offsets


def parse_capture(
    capture: Path,
    *,
    expected_bursts: int | None,
    expected_payload_sha256: str | None,
    burst_period_bytes: int,
    allow_zero_prefix_suffix: bool,
    write_payload: Path | None,
    capture_start_monotonic_ns: int | None = None,
    capture_end_monotonic_ns: int | None = None,
    capture_reset_count: int | None = None,
    capture_drop_count: int | None = None,
    require_capture_metadata: bool = False,
) -> GateReport:
    data = capture.read_bytes()
    failures: list[str] = []
    burst_evidence: list[BurstEvidence] = []
    payloads: list[bytes] = []

    if burst_period_bytes < 16 or burst_period_bytes % 2:
        raise GateFailure(f"invalid IEC61937 repetition period: {burst_period_bytes}")

    first = data.find(SYNC)
    if first < 0:
        raise GateFailure("no IEC61937 Pa/Pb sync preamble found")

    prefix = data[:first]
    zero_prefix = not any(prefix)
    if first and (not allow_zero_prefix_suffix or not zero_prefix):
        failures.append(f"capture has {first} non-admitted prefix bytes before first IEC61937 burst")

    offsets = consecutive_burst_offsets(data, first, burst_period_bytes)
    if expected_bursts is not None and len(offsets) != expected_bursts:
        failures.append(f"burst count mismatch: expected {expected_bursts}, got {len(offsets)} consecutive bursts")

    train_end = first + len(offsets) * burst_period_bytes
    suffix = data[train_end:] if train_end <= len(data) else b""
    zero_suffix = not any(suffix)
    if suffix and (not allow_zero_prefix_suffix or not zero_suffix):
        next_sync = suffix.find(SYNC)
        detail = ""
        if next_sync >= 0:
            detail = f"; next sync appears {next_sync} bytes into suffix (transport discontinuity/byte slip)"
        failures.append(f"capture has {len(suffix)} non-admitted suffix bytes after burst train{detail}")

    if require_capture_metadata:
        missing = []
        if capture_start_monotonic_ns is None:
            missing.append("capture_start_monotonic_ns")
        if capture_end_monotonic_ns is None:
            missing.append("capture_end_monotonic_ns")
        if capture_reset_count is None:
            missing.append("capture_reset_count")
        if capture_drop_count is None:
            missing.append("capture_drop_count")
        if missing:
            failures.append("required physical capture metadata missing: " + ", ".join(missing))

    if capture_start_monotonic_ns is not None and capture_end_monotonic_ns is not None:
        if capture_end_monotonic_ns <= capture_start_monotonic_ns:
            failures.append("capture end monotonic timestamp is not after capture start")
    if capture_reset_count is not None and capture_reset_count != 0:
        failures.append(f"capture reset counter is non-zero: {capture_reset_count}")
    if capture_drop_count is not None and capture_drop_count != 0:
        failures.append(f"capture drop counter is non-zero: {capture_drop_count}")

    for index, offset in enumerate(offsets):
        end = offset + burst_period_bytes
        if end > len(data):
            failures.append(
                f"truncated burst {index}: needs {burst_period_bytes} bytes from offset {offset}, capture ends at {len(data)}"
            )
            break
        burst = data[offset:end]
        pa, pb, pc, pd = struct.unpack_from("<HHHH", burst, 0)
        if pa != PA or pb != PB:
            failures.append(f"burst {index} invalid Pa/Pb: 0x{pa:04x}/0x{pb:04x}")
            continue

        data_type = pc & 0x7F
        error_flag = bool(pc & 0x80)
        if data_type != EAC3_TYPE:
            failures.append(f"burst {index} data type is 0x{data_type:02x}, expected 0x15")
        if error_flag:
            failures.append(f"burst {index} has IEC61937 Pc error flag set")

        payload_bytes = int(pd)  # For IEC61937 E-AC-3, Pd is a byte count.
        if payload_bytes <= 0:
            failures.append(f"burst {index} has empty E-AC-3 payload")
            continue
        if 8 + payload_bytes > burst_period_bytes:
            failures.append(
                f"burst {index} Pd={payload_bytes} exceeds {burst_period_bytes}-byte repetition period"
            )
            continue
        if payload_bytes % 2:
            failures.append(f"burst {index} Pd={payload_bytes} is odd; exact 16-bit word reconstruction is impossible")
            continue

        framed_payload = burst[8 : 8 + payload_bytes]
        raw_payload = swap16(framed_payload)
        payload_syncword_ok = len(raw_payload) >= 2 and raw_payload[:2] == b"\x0b\x77"
        if not payload_syncword_ok:
            failures.append(f"burst {index} reconstructed payload does not begin with E-AC-3 syncword 0x0b77")

        padding = burst[8 + payload_bytes :]
        padding_zero = not any(padding)
        if not padding_zero:
            failures.append(f"burst {index} contains non-zero bytes in transport padding")

        payloads.append(raw_payload)
        burst_evidence.append(
            BurstEvidence(
                index=index,
                offset=offset,
                pc=pc,
                data_type=data_type,
                error_flag=error_flag,
                payload_bytes=payload_bytes,
                payload_sha256=sha256_bytes(raw_payload),
                payload_syncword_ok=payload_syncword_ok,
                padding_bytes=len(padding),
                padding_zero=padding_zero,
            )
        )

    reconstructed = b"".join(payloads)
    reconstructed_sha = sha256_bytes(reconstructed)
    identity_match: bool | None = None
    if expected_payload_sha256:
        identity_match = reconstructed_sha.lower() == expected_payload_sha256.lower()
        if not identity_match:
            failures.append(
                "reconstructed E-AC-3 SHA-256 mismatch: "
                f"expected {expected_payload_sha256.lower()}, got {reconstructed_sha}"
            )

    if write_payload is not None and reconstructed:
        write_payload.parent.mkdir(parents=True, exist_ok=True)
        write_payload.write_bytes(reconstructed)

    duration_ns = None
    if capture_start_monotonic_ns is not None and capture_end_monotonic_ns is not None:
        duration_ns = capture_end_monotonic_ns - capture_start_monotonic_ns

    all_padding_zero = len(burst_evidence) == len(offsets) and all(item.padding_zero for item in burst_evidence)
    verdict = "pass" if not failures else "fail"
    return GateReport(
        schema_version=1,
        verdict=verdict,
        capture_path=str(capture),
        capture_bytes=len(data),
        capture_sha256=sha256_file(capture),
        capture_start_monotonic_ns=capture_start_monotonic_ns,
        capture_end_monotonic_ns=capture_end_monotonic_ns,
        capture_duration_ns=duration_ns,
        capture_reset_count=capture_reset_count,
        capture_drop_count=capture_drop_count,
        prefix_bytes=len(prefix),
        suffix_bytes=len(suffix),
        burst_period_bytes=burst_period_bytes,
        burst_count=len(offsets),
        expected_burst_count=expected_bursts,
        data_type=EAC3_TYPE,
        reconstructed_payload_bytes=len(reconstructed),
        reconstructed_payload_sha256=reconstructed_sha,
        expected_payload_sha256=expected_payload_sha256.lower() if expected_payload_sha256 else None,
        payload_identity_match=identity_match,
        zero_prefix=zero_prefix,
        zero_suffix=zero_suffix,
        all_padding_zero=all_padding_zero,
        failures=failures,
        bursts=[asdict(item) for item in burst_evidence],
        truth_boundary=(
            "Physical IEC61937 ingress/capture evidence only. A pass does not prove Aurora physical "
            "12-channel output, DAC/analog behavior, DRM-service compatibility, authored-position "
            "correctness, Dolby certification, or acoustic parity."
        ),
    )


def build_synthetic_capture(payloads: list[bytes], *, prefix: bytes = b"", suffix: bytes = b"") -> bytes:
    out = bytearray(prefix)
    for payload in payloads:
        if len(payload) % 2:
            raise ValueError("synthetic payload must be even length")
        if not payload.startswith(b"\x0b\x77"):
            raise ValueError("synthetic payload must begin with E-AC-3 syncword")
        framed = swap16(payload)
        preamble = struct.pack("<HHHH", PA, PB, EAC3_TYPE, len(payload))
        if len(preamble) + len(framed) > EAC3_BURST_BYTES:
            raise ValueError("synthetic payload too large")
        out.extend(preamble)
        out.extend(framed)
        out.extend(bytes(EAC3_BURST_BYTES - len(preamble) - len(framed)))
    out.extend(suffix)
    return bytes(out)


def self_test() -> int:
    import tempfile

    payloads = [b"\x0b\x77" + bytes([index]) * (1022 + 2 * index) for index in range(1, 4)]
    expected = sha256_bytes(b"".join(payloads))
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        good = root / "good.spdif"
        good.write_bytes(build_synthetic_capture(payloads, prefix=b"\x00" * 64, suffix=b"\x00" * 32))
        report = parse_capture(
            good,
            expected_bursts=3,
            expected_payload_sha256=expected,
            burst_period_bytes=EAC3_BURST_BYTES,
            allow_zero_prefix_suffix=True,
            write_payload=root / "reconstructed.ec3",
            capture_start_monotonic_ns=1_000_000,
            capture_end_monotonic_ns=2_000_000,
            capture_reset_count=0,
            capture_drop_count=0,
            require_capture_metadata=True,
        )
        if report.verdict != "pass" or report.reconstructed_payload_sha256 != expected:
            raise AssertionError(report)

        first = 64
        cases: list[tuple[str, bytearray]] = []

        bad_type = bytearray(good.read_bytes())
        struct.pack_into("<H", bad_type, first + 4, 0x01)
        cases.append(("wrong-data-type", bad_type))

        bad_payload = bytearray(good.read_bytes())
        bad_payload[first + 10] ^= 0x01
        cases.append(("payload-mutation", bad_payload))

        bad_padding = bytearray(good.read_bytes())
        bad_padding[first + 20000] = 1
        cases.append(("nonzero-padding", bad_padding))

        byte_slip = bytearray(good.read_bytes())
        del byte_slip[first + 20000]
        cases.append(("byte-slip", byte_slip))

        for name, capture_bytes in cases:
            path = root / f"{name}.spdif"
            path.write_bytes(capture_bytes)
            bad = parse_capture(
                path,
                expected_bursts=3,
                expected_payload_sha256=expected,
                burst_period_bytes=EAC3_BURST_BYTES,
                allow_zero_prefix_suffix=True,
                write_payload=None,
            )
            if bad.verdict != "fail":
                raise AssertionError(f"{name} did not fail closed")

        reset_fail = parse_capture(
            good,
            expected_bursts=3,
            expected_payload_sha256=expected,
            burst_period_bytes=EAC3_BURST_BYTES,
            allow_zero_prefix_suffix=True,
            write_payload=None,
            capture_start_monotonic_ns=1,
            capture_end_monotonic_ns=2,
            capture_reset_count=1,
            capture_drop_count=0,
            require_capture_metadata=True,
        )
        if reset_fail.verdict != "fail":
            raise AssertionError("non-zero capture reset counter did not fail closed")

    print("AURORA-PHYSICAL-INGRESS-SELFTEST-PASS")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    analyze = sub.add_parser("analyze", help="validate one captured IEC61937 E-AC-3 byte stream")
    analyze.add_argument("--capture", type=Path, required=True)
    analyze.add_argument("--report", type=Path, required=True)
    analyze.add_argument("--write-payload", type=Path)
    analyze.add_argument("--expected-bursts", type=int, default=2360)
    analyze.add_argument("--expected-payload-sha256", default=PINNED_MOVING_SHA256)
    analyze.add_argument("--burst-period-bytes", type=int, default=EAC3_BURST_BYTES)
    analyze.add_argument("--capture-start-monotonic-ns", type=int)
    analyze.add_argument("--capture-end-monotonic-ns", type=int)
    analyze.add_argument("--capture-reset-count", type=int)
    analyze.add_argument("--capture-drop-count", type=int)
    analyze.add_argument(
        "--require-capture-metadata",
        action="store_true",
        help="require start/end monotonic timestamps plus explicit reset/drop counters",
    )
    analyze.add_argument(
        "--allow-zero-prefix-suffix",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="allow zero-only capture lead-in/trail-out around the exact burst train (default: true)",
    )

    sub.add_parser("self-test", help="exercise positive and fail-closed synthetic capture cases")
    args = parser.parse_args()

    if args.command == "self-test":
        return self_test()

    if not args.capture.is_file():
        raise SystemExit(f"capture does not exist: {args.capture}")
    report = parse_capture(
        args.capture,
        expected_bursts=args.expected_bursts,
        expected_payload_sha256=args.expected_payload_sha256 or None,
        burst_period_bytes=args.burst_period_bytes,
        allow_zero_prefix_suffix=args.allow_zero_prefix_suffix,
        write_payload=args.write_payload,
        capture_start_monotonic_ns=args.capture_start_monotonic_ns,
        capture_end_monotonic_ns=args.capture_end_monotonic_ns,
        capture_reset_count=args.capture_reset_count,
        capture_drop_count=args.capture_drop_count,
        require_capture_metadata=args.require_capture_metadata,
    )
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(asdict(report), indent=2, sort_keys=True) + "\n", encoding="utf-8")

    print(
        f"AURORA-PHYSICAL-INGRESS-{report.verdict.upper()} "
        f"bursts={report.burst_count} payload_bytes={report.reconstructed_payload_bytes} "
        f"payload_sha256={report.reconstructed_payload_sha256}"
    )
    if report.failures:
        for failure in report.failures:
            print(f"FAIL: {failure}", file=sys.stderr)
    print(f"report={args.report}")
    return 0 if report.verdict == "pass" else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except GateFailure as exc:
        print(f"AURORA-PHYSICAL-INGRESS-FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
