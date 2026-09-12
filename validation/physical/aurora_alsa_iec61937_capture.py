#!/usr/bin/env python3
"""Capture/convert a Linux ALSA S32_LE stereo 192 kHz stream into canonical IEC61937 bytes.

This adapter is intentionally hardware-neutral. It never hard-codes a Raspberry Pi
card/device name and it never invents hardware reset/drop counters. The operator
must select the ALSA device explicitly. When real hardware telemetry provides reset
or drop counters, pass those values to the downstream physical ingress validator.

Expected encoded-ingress shape:
- ALSA: S32_LE, 2 channels, 192000 Hz;
- one 16-bit IEC60958/61937 word embedded in each 32-bit ALSA sample slot;
- stereo sample order carries successive IEC words;
- useful 16 bits may occupy the high or low half of the 32-bit slot depending on
  the capture driver; channel order may be LR or RL.

The converter probes only those four explicit possibilities (high/low x LR/RL) and
chooses the unique candidate with the strongest exact 24576-byte IEC61937 burst
train. It does not guess arbitrary bit shifts. Ambiguous or sync-free captures fail.

A pass here proves only deterministic conversion of a selected ALSA capture into a
canonical byte stream. Physical Gate A still requires `aurora_physical_ingress.py`
with real monotonic timestamps and explicit hardware reset/drop counters.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path

SYNC = bytes.fromhex("72f81f4e")
BURST_BYTES = 24576
SAMPLE_BYTES = 4
CHANNELS = 2
FRAME_BYTES = SAMPLE_BYTES * CHANNELS
RATE_HZ = 192000
FORMAT = "S32_LE"


class CaptureError(RuntimeError):
    pass


@dataclass(frozen=True)
class CandidateScore:
    word_lane: str
    channel_order: str
    first_sync_offset: int
    consecutive_bursts: int
    total_sync_occurrences: int
    canonical_bytes: int


@dataclass
class ConversionReport:
    schema_version: int
    input_path: str
    input_bytes: int
    input_sha256: str
    output_path: str
    output_bytes: int
    output_sha256: str
    sample_format: str
    channels: int
    sample_rate_hz: int
    selected_word_lane: str
    selected_channel_order: str
    first_sync_offset: int
    consecutive_bursts: int
    total_sync_occurrences: int
    candidate_scores: list[dict]
    truth_boundary: str


@dataclass
class CaptureReport:
    schema_version: int
    device: str
    sample_format: str
    channels: int
    sample_rate_hz: int
    requested_seconds: float
    capture_start_monotonic_ns: int
    capture_end_monotonic_ns: int
    capture_duration_ns: int
    arecord_exit_code: int
    arecord_xrun_marker_count: int
    raw_capture_path: str
    raw_capture_bytes: int
    raw_capture_sha256: str
    canonical_iec_path: str | None
    canonical_iec_sha256: str | None
    selected_word_lane: str | None
    selected_channel_order: str | None
    consecutive_bursts: int | None
    operator_hardware_reset_count: int | None
    operator_hardware_drop_count: int | None
    hardware_counter_source: str | None
    hw_params_log: str
    capture_stderr_log: str
    truth_boundary: str


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def count_occurrences(data: bytes, needle: bytes) -> int:
    count = 0
    start = 0
    while True:
        pos = data.find(needle, start)
        if pos < 0:
            return count
        count += 1
        start = pos + 1


def extract_words(raw: bytes, *, word_lane: str, channel_order: str) -> bytes:
    if len(raw) % FRAME_BYTES:
        raise CaptureError(
            f"raw S32_LE stereo capture length {len(raw)} is not a multiple of {FRAME_BYTES} bytes"
        )
    if word_lane not in {"high16", "low16"}:
        raise ValueError(word_lane)
    if channel_order not in {"lr", "rl"}:
        raise ValueError(channel_order)

    shift = 16 if word_lane == "high16" else 0
    out = bytearray(len(raw) // 2)
    out_offset = 0
    for frame_offset in range(0, len(raw), FRAME_BYTES):
        left = struct.unpack_from("<I", raw, frame_offset)[0]
        right = struct.unpack_from("<I", raw, frame_offset + SAMPLE_BYTES)[0]
        words = ((left >> shift) & 0xFFFF, (right >> shift) & 0xFFFF)
        if channel_order == "rl":
            words = (words[1], words[0])
        struct.pack_into("<HH", out, out_offset, words[0], words[1])
        out_offset += 4
    return bytes(out)


def score_candidate(data: bytes, *, word_lane: str, channel_order: str) -> tuple[CandidateScore, bytes]:
    canonical = extract_words(data, word_lane=word_lane, channel_order=channel_order)
    first = canonical.find(SYNC)
    total = count_occurrences(canonical, SYNC)
    consecutive = 0
    if first >= 0:
        offset = first
        while offset + 8 <= len(canonical) and canonical[offset : offset + 4] == SYNC:
            consecutive += 1
            offset += BURST_BYTES
    return (
        CandidateScore(
            word_lane=word_lane,
            channel_order=channel_order,
            first_sync_offset=first,
            consecutive_bursts=consecutive,
            total_sync_occurrences=total,
            canonical_bytes=len(canonical),
        ),
        canonical,
    )


def select_candidate(raw: bytes, *, word_lane: str, channel_order: str) -> tuple[CandidateScore, bytes, list[CandidateScore]]:
    lanes = [word_lane] if word_lane != "auto" else ["high16", "low16"]
    orders = [channel_order] if channel_order != "auto" else ["lr", "rl"]
    scored: list[tuple[CandidateScore, bytes]] = []
    for lane in lanes:
        for order in orders:
            scored.append(score_candidate(raw, word_lane=lane, channel_order=order))

    scored.sort(
        key=lambda item: (
            item[0].consecutive_bursts,
            item[0].total_sync_occurrences,
            -item[0].first_sync_offset if item[0].first_sync_offset >= 0 else -(1 << 62),
        ),
        reverse=True,
    )
    best, canonical = scored[0]
    if best.consecutive_bursts <= 0:
        summary = ", ".join(
            f"{s.word_lane}/{s.channel_order}:syncs={s.total_sync_occurrences}" for s, _ in scored
        )
        raise CaptureError(f"no valid IEC61937 burst train found in any allowed S32_LE interpretation ({summary})")

    # If two distinct interpretations are equally plausible at the exact burst-grid
    # level, fail rather than silently selecting one.
    ties = [
        s
        for s, _ in scored
        if s.consecutive_bursts == best.consecutive_bursts
        and s.total_sync_occurrences == best.total_sync_occurrences
        and s.first_sync_offset == best.first_sync_offset
    ]
    if len(ties) > 1:
        names = ", ".join(f"{s.word_lane}/{s.channel_order}" for s in ties)
        raise CaptureError(f"ambiguous ALSA word interpretation; tied candidates: {names}")

    return best, canonical, [s for s, _ in scored]


def convert_file(
    input_path: Path,
    output_path: Path,
    report_path: Path,
    *,
    word_lane: str,
    channel_order: str,
) -> ConversionReport:
    raw = input_path.read_bytes()
    best, canonical, scores = select_candidate(raw, word_lane=word_lane, channel_order=channel_order)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_bytes(canonical)
    report = ConversionReport(
        schema_version=1,
        input_path=str(input_path),
        input_bytes=len(raw),
        input_sha256=sha256_file(input_path),
        output_path=str(output_path),
        output_bytes=len(canonical),
        output_sha256=sha256_file(output_path),
        sample_format=FORMAT,
        channels=CHANNELS,
        sample_rate_hz=RATE_HZ,
        selected_word_lane=best.word_lane,
        selected_channel_order=best.channel_order,
        first_sync_offset=best.first_sync_offset,
        consecutive_bursts=best.consecutive_bursts,
        total_sync_occurrences=best.total_sync_occurrences,
        candidate_scores=[asdict(item) for item in scores],
        truth_boundary=(
            "ALSA slot-to-IEC61937 conversion evidence only. This does not prove eARC electrical integrity, "
            "hardware reset/drop counters, Aurora rendering, physical multichannel output, or Dolby conformance."
        ),
    )
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(asdict(report), indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return report


def xrun_markers(text: str) -> int:
    # ALSA tools/locales vary. Keep this a diagnostic count, never promote it to the
    # hardware drop counter required by Gate A.
    return len(re.findall(r"(?i)\b(?:xrun|overrun|underrun)\b", text))


def dump_hw_params(arecord: str, device: str, log_path: Path) -> None:
    command = [
        arecord,
        "-D",
        device,
        "-f",
        FORMAT,
        "-c",
        str(CHANNELS),
        "-r",
        str(RATE_HZ),
        "--dump-hw-params",
        "-d",
        "1",
        "-t",
        "raw",
        os.devnull,
    ]
    result = subprocess.run(command, text=True, capture_output=True)
    log_path.parent.mkdir(parents=True, exist_ok=True)
    log_path.write_text(
        "$ " + subprocess.list2cmdline(command) + "\n\nSTDOUT\n" + result.stdout + "\nSTDERR\n" + result.stderr,
        encoding="utf-8",
        errors="replace",
    )
    if result.returncode != 0:
        raise CaptureError(f"arecord hardware-parameter probe failed with exit code {result.returncode}; see {log_path}")


def capture_alsa(
    *,
    device: str,
    seconds: float,
    raw_out: Path,
    iec_out: Path,
    metadata_out: Path,
    conversion_report: Path,
    hw_params_log: Path,
    stderr_log: Path,
    word_lane: str,
    channel_order: str,
    hardware_reset_count: int | None,
    hardware_drop_count: int | None,
    hardware_counter_source: str | None,
) -> CaptureReport:
    arecord = shutil.which("arecord")
    if not arecord:
        raise CaptureError("arecord not found; install alsa-utils on the Linux capture host")
    if seconds <= 0:
        raise CaptureError("capture duration must be positive")
    if (hardware_reset_count is None) != (hardware_drop_count is None):
        raise CaptureError("hardware reset/drop counters must be supplied together or both omitted")
    if hardware_reset_count is not None and not hardware_counter_source:
        raise CaptureError("--hardware-counter-source is required when reset/drop counters are supplied")

    raw_out.parent.mkdir(parents=True, exist_ok=True)
    dump_hw_params(arecord, device, hw_params_log)

    # arecord accepts integer seconds. Round up so a requested 80.1 s can never
    # silently capture only 80 s.
    duration_s = max(1, int(seconds) if float(seconds).is_integer() else int(seconds) + 1)
    command = [
        arecord,
        "-D",
        device,
        "-f",
        FORMAT,
        "-c",
        str(CHANNELS),
        "-r",
        str(RATE_HZ),
        "-t",
        "raw",
        "-d",
        str(duration_s),
        str(raw_out),
    ]
    start_ns = time.monotonic_ns()
    result = subprocess.run(command, text=True, capture_output=True)
    end_ns = time.monotonic_ns()
    stderr_log.parent.mkdir(parents=True, exist_ok=True)
    stderr_log.write_text(result.stderr, encoding="utf-8", errors="replace")

    report = CaptureReport(
        schema_version=1,
        device=device,
        sample_format=FORMAT,
        channels=CHANNELS,
        sample_rate_hz=RATE_HZ,
        requested_seconds=seconds,
        capture_start_monotonic_ns=start_ns,
        capture_end_monotonic_ns=end_ns,
        capture_duration_ns=end_ns - start_ns,
        arecord_exit_code=result.returncode,
        arecord_xrun_marker_count=xrun_markers(result.stderr),
        raw_capture_path=str(raw_out),
        raw_capture_bytes=raw_out.stat().st_size if raw_out.exists() else 0,
        raw_capture_sha256=sha256_file(raw_out) if raw_out.exists() else "",
        canonical_iec_path=None,
        canonical_iec_sha256=None,
        selected_word_lane=None,
        selected_channel_order=None,
        consecutive_bursts=None,
        operator_hardware_reset_count=hardware_reset_count,
        operator_hardware_drop_count=hardware_drop_count,
        hardware_counter_source=hardware_counter_source,
        hw_params_log=str(hw_params_log),
        capture_stderr_log=str(stderr_log),
        truth_boundary=(
            "Capture-adapter metadata. arecord xrun-marker count is diagnostic only and is not a substitute for "
            "a hardware/driver reset/drop counter required by physical Gate A."
        ),
    )

    if result.returncode == 0 and raw_out.exists() and raw_out.stat().st_size:
        converted = convert_file(
            raw_out,
            iec_out,
            conversion_report,
            word_lane=word_lane,
            channel_order=channel_order,
        )
        report.canonical_iec_path = str(iec_out)
        report.canonical_iec_sha256 = converted.output_sha256
        report.selected_word_lane = converted.selected_word_lane
        report.selected_channel_order = converted.selected_channel_order
        report.consecutive_bursts = converted.consecutive_bursts

    metadata_out.parent.mkdir(parents=True, exist_ok=True)
    metadata_out.write_text(json.dumps(asdict(report), indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if result.returncode != 0:
        raise CaptureError(f"arecord capture failed with exit code {result.returncode}; metadata={metadata_out}")
    if report.arecord_xrun_marker_count:
        raise CaptureError(
            f"arecord reported {report.arecord_xrun_marker_count} xrun/overrun/underrun markers; metadata={metadata_out}"
        )
    return report


def canonical_to_s32(canonical: bytes, *, word_lane: str, channel_order: str) -> bytes:
    if len(canonical) % 4:
        raise ValueError("canonical test stream must contain whole stereo word frames")
    shift = 16 if word_lane == "high16" else 0
    out = bytearray((len(canonical) // 4) * FRAME_BYTES)
    out_off = 0
    for off in range(0, len(canonical), 4):
        w0, w1 = struct.unpack_from("<HH", canonical, off)
        if channel_order == "rl":
            left, right = w1, w0
        else:
            left, right = w0, w1
        struct.pack_into("<II", out, out_off, left << shift, right << shift)
        out_off += FRAME_BYTES
    return bytes(out)


def synthetic_iec(bursts: int = 3) -> bytes:
    out = bytearray()
    for index in range(bursts):
        payload = b"\x77\x0b" + bytes([index + 1]) * 1022  # framed word-swapped payload is enough for conversion test.
        preamble = SYNC + struct.pack("<HH", 0x15, len(payload))
        burst = preamble + payload
        out.extend(burst)
        out.extend(bytes(BURST_BYTES - len(burst)))
    return bytes(out)


def self_test() -> int:
    canonical = synthetic_iec(3)
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        for lane, order in (("high16", "lr"), ("high16", "rl"), ("low16", "lr"), ("low16", "rl")):
            raw_path = root / f"{lane}-{order}.raw"
            out_path = root / f"{lane}-{order}.spdif"
            report_path = root / f"{lane}-{order}.json"
            raw_path.write_bytes(canonical_to_s32(canonical, word_lane=lane, channel_order=order))
            report = convert_file(raw_path, out_path, report_path, word_lane="auto", channel_order="auto")
            if out_path.read_bytes() != canonical:
                raise AssertionError(f"round-trip mismatch for {lane}/{order}")
            if (report.selected_word_lane, report.selected_channel_order) != (lane, order):
                raise AssertionError(
                    f"auto-detection mismatch: encoded={lane}/{order} selected={report.selected_word_lane}/{report.selected_channel_order}"
                )
            if report.consecutive_bursts != 3:
                raise AssertionError(report)

        garbage = root / "garbage.raw"
        garbage.write_bytes(bytes(FRAME_BYTES * 1000))
        try:
            convert_file(garbage, root / "garbage.spdif", root / "garbage.json", word_lane="auto", channel_order="auto")
        except CaptureError:
            pass
        else:
            raise AssertionError("sync-free raw capture did not fail closed")

    print("AURORA-ALSA-IEC61937-CAPTURE-SELFTEST-PASS")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    convert = sub.add_parser("convert", help="convert an existing S32_LE 2ch 192k raw capture")
    convert.add_argument("--input", type=Path, required=True)
    convert.add_argument("--output", type=Path, required=True)
    convert.add_argument("--report", type=Path, required=True)
    convert.add_argument("--word-lane", choices=["auto", "high16", "low16"], default="auto")
    convert.add_argument("--channel-order", choices=["auto", "lr", "rl"], default="auto")

    capture = sub.add_parser("capture", help="capture from an explicitly selected ALSA device and convert it")
    capture.add_argument("--device", required=True, help="explicit ALSA PCM device, e.g. hw:CARD,DEV")
    capture.add_argument("--seconds", type=float, default=85.0)
    capture.add_argument("--raw-out", type=Path, required=True)
    capture.add_argument("--iec-out", type=Path, required=True)
    capture.add_argument("--metadata", type=Path, required=True)
    capture.add_argument("--conversion-report", type=Path, required=True)
    capture.add_argument("--hw-params-log", type=Path, required=True)
    capture.add_argument("--stderr-log", type=Path, required=True)
    capture.add_argument("--word-lane", choices=["auto", "high16", "low16"], default="auto")
    capture.add_argument("--channel-order", choices=["auto", "lr", "rl"], default="auto")
    capture.add_argument("--hardware-reset-count", type=int)
    capture.add_argument("--hardware-drop-count", type=int)
    capture.add_argument("--hardware-counter-source")

    sub.add_parser("self-test", help="test high/low word placement, LR/RL order, and fail-closed sync detection")
    args = parser.parse_args()

    if args.command == "self-test":
        return self_test()

    if args.command == "convert":
        if not args.input.is_file():
            raise CaptureError(f"input capture does not exist: {args.input}")
        report = convert_file(
            args.input,
            args.output,
            args.report,
            word_lane=args.word_lane,
            channel_order=args.channel_order,
        )
        print(
            "AURORA-ALSA-IEC61937-CONVERT-PASS "
            f"lane={report.selected_word_lane} order={report.selected_channel_order} "
            f"bursts={report.consecutive_bursts} output={report.output_path}"
        )
        return 0

    report = capture_alsa(
        device=args.device,
        seconds=args.seconds,
        raw_out=args.raw_out,
        iec_out=args.iec_out,
        metadata_out=args.metadata,
        conversion_report=args.conversion_report,
        hw_params_log=args.hw_params_log,
        stderr_log=args.stderr_log,
        word_lane=args.word_lane,
        channel_order=args.channel_order,
        hardware_reset_count=args.hardware_reset_count,
        hardware_drop_count=args.hardware_drop_count,
        hardware_counter_source=args.hardware_counter_source,
    )
    print(
        "AURORA-ALSA-IEC61937-CAPTURE-PASS "
        f"device={report.device} lane={report.selected_word_lane} order={report.selected_channel_order} "
        f"bursts={report.consecutive_bursts} metadata={args.metadata}"
    )
    if report.operator_hardware_reset_count is None:
        print(
            "NOTE: hardware reset/drop counters are still unknown; do not run Gate A with --require-capture-metadata "
            "until real counter values/source are available.",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except CaptureError as exc:
        print(f"AURORA-ALSA-IEC61937-CAPTURE-FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
