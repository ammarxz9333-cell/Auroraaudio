#!/usr/bin/env python3
"""Validate a physical IEC61937 E-AC-3 capture for Aurora issue #143.

The gate is intentionally narrow and fail-closed. It validates the physical capture
format before that capture is admitted to the already-proven Aurora moving-JOC
software path.

For E-AC-3 (IEC61937 data type 0x15), FFmpeg-compatible IEC61937 framing uses:
- Pa/Pb sync words 0xF872 / 0x4E1F;
- Pc low 7 bits = 0x15;
- Pc bit 7 = error flag (must be zero for acceptance);
- Pd = E-AC-3 payload length in bytes;
- one 24576-byte repetition period at the transport byte-stream level;
- payload words byte-swapped in the little-endian IEC byte stream.

The validator reconstructs the concatenated raw E-AC-3 payload, optionally checks
its SHA-256 against Aurora's pinned carrier, verifies exact burst spacing, and
requires zero-only transport padding outside payload bytes.

Passing this program is capture/transport evidence only. It is not physical output,
Dolby conformance/certification, or DRM-service compatibility evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from dataclasses import dataclass, asdict
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
    padding_bytes: int


@dataclass
class GateReport:
    schema_version: int
    verdict: str
    capture_path: str
    capture_bytes: int
    capture_sha256: str
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
        raise GateFailure(f"E-AC-3 payload length must be even for 16-bit IEC word swap, got {len(data)}")
    out = bytearray(len(data))
    out[0::2] = data[1::2]
    out[1::2] = data[0::2]
    return bytes(out)


def find_sync_offsets(data: bytes) -> list[int]:
    offsets: list[int] = []
    start = 0
    while True:
        pos = data.find(SYNC, start)
        if pos < 0:
            return offsets
        offsets.append(pos)
        start = pos + 1


def parse_capture(
    capture: Path,
    *,
    expected_bursts: int | None,
    expected_payload_sha256: str | None,
    burst_period_bytes: int,
    allow_zero_prefix_suffix: bool,
    write_payload: Path | None,
) -> GateReport:
    data = capture.read_bytes()
    failures: list[str] = []
    burst_evidence: list[BurstEvidence] = []
    payloads: list[bytes] = []

    sync_offsets = find_sync_offsets(data)
    if not sync_offsets:
        raise GateFailure("no IEC61937 Pa/Pb sync preamble found")

    first = sync_offsets[0]
    prefix = data[:first]
    zero_prefix = not any(prefix)
    if first and (not allow_zero_prefix_suffix or not zero_prefix):
        failures.append(f"capture has {first} non-admitted prefix bytes before first IEC61937 burst")

    # Every detected sync must lie on the exact repetition-period grid. This catches
    # inserted/dropped bytes and extra false sync patterns inside padding/payload.
    for index, offset in enumerate(sync_offsets):
        expected_offset = first + index * burst_period_bytes
        if offset != expected_offset:
            failures.append(
                f"burst spacing discontinuity at detected burst {index}: expected offset {expected_offset}, got {offset}"
            )
            break

    if expected_bursts is not None and len(sync_offsets) != expected_bursts:
        failures.append(f"burst count mismatch: expected {expected_bursts}, got {len(sync_offsets)}")

    for index, offset in enumerate(sync_offsets):
        end = offset + burst_period_bytes
        if end > len(data):
            failures.append(
                f"truncated burst {index}: needs {burst_period_bytes} bytes from offset {offset}, capture ends at {len(data)}"
            )
            break
        burst = data[offset:end]
        if len(burst) < 8:
            failures.append(f"burst {index} shorter than IEC61937 preamble")
            break

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

        payload_bytes = int(pd)  # IEC61937 E-AC-3 Pd is byte-count, not bit-count.
        if payload_bytes <= 0:
            failures.append(f"burst {index} has empty E-AC-3 payload")
            continue
        if 8 + payload_bytes > burst_period_bytes:
            failures.append(
                f"burst {index} Pd={payload_bytes} exceeds {burst_period_bytes}-byte repetition period"
            )
            continue
        if payload_bytes % 2:
            failures.append(f"burst {index} Pd={payload_bytes} is odd; cannot reverse IEC 16-bit word swap exactly")
            continue

        framed_payload = burst[8 : 8 + payload_bytes]
        raw_payload = swap16(framed_payload)
        if len(raw_payload) < 2 or raw_payload[:2] != b"\x0b\x77":
            failures.append(
                f"burst {index} reconstructed payload does not begin with E-AC-3 syncword 0x0b77"
            )

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
                padding_bytes=len(padding),
            )
        )

    # A complete capture may contain zero-only lead-in/trail-out around the exact
    # burst train. Non-zero trailing bytes are never admitted by the physical gate.
    train_end = first + len(sync_offsets) * burst_period_bytes
    suffix = data[train_end:] if train_end <= len(data) else b""
    zero_suffix = not any(suffix)
    if suffix and (not allow_zero_prefix_suffix or not zero_suffix):
        failures.append(f"capture has {len(suffix)} non-admitted suffix bytes after final burst")

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

    all_padding_zero = len(burst_evidence) == len(sync_offsets) and not any(
        "transport padding" in failure for failure in failures
    )
    verdict = "pass" if not failures else "fail"
    return GateReport(
        schema_version=1,
        verdict=verdict,
        capture_path=str(capture),
        capture_bytes=len(data),
        capture_sha256=sha256_file(capture),
        prefix_bytes=len(prefix),
        suffix_bytes=len(suffix),
        burst_period_bytes=burst_period_bytes,
        burst_count=len(sync_offsets),
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
            "Physical IEC61937 capture/transport evidence only. A pass does not prove Aurora physical "
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

    payloads = [
        b"\x0b\x77" + bytes([index]) * (1022 + 2 * index)
        for index in range(1, 4)
    ]
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
        )
        if report.verdict != "pass" or report.reconstructed_payload_sha256 != expected:
            raise AssertionError(report)

        bad_type = bytearray(good.read_bytes())
        first = 64
        struct.pack_into("<H", bad_type, first + 4, 0x01)
        bad_type_path = root / "bad-type.spdif"
        bad_type_path.write_bytes(bad_type)
        if parse_capture(
            bad_type_path,
            expected_bursts=3,
            expected_payload_sha256=expected,
            burst_period_bytes=EAC3_BURST_BYTES,
            allow_zero_prefix_suffix=True,
            write_payload=None,
        ).verdict != "fail":
            raise AssertionError("wrong data type did not fail closed")

        bad_payload = bytearray(good.read_bytes())
        bad_payload[first + 10] ^= 0x01
        bad_payload_path = root / "bad-payload.spdif"
        bad_payload_path.write_bytes(bad_payload)
        if parse_capture(
            bad_payload_path,
            expected_bursts=3,
            expected_payload_sha256=expected,
            burst_period_bytes=EAC3_BURST_BYTES,
            allow_zero_prefix_suffix=True,
            write_payload=None,
        ).verdict != "fail":
            raise AssertionError("payload mutation did not fail closed")

        bad_padding = bytearray(good.read_bytes())
        bad_padding[first + 20000] = 1
        bad_padding_path = root / "bad-padding.spdif"
        bad_padding_path.write_bytes(bad_padding)
        if parse_capture(
            bad_padding_path,
            expected_bursts=3,
            expected_payload_sha256=expected,
            burst_period_bytes=EAC3_BURST_BYTES,
            allow_zero_prefix_suffix=True,
            write_payload=None,
        ).verdict != "fail":
            raise AssertionError("non-zero transport padding did not fail closed")

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
