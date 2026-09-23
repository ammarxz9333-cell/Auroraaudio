#!/usr/bin/env python3
"""Continuous ALSA S32_LE -> canonical IEC61937 stream adapter for Aurora.

The fixed-duration capture adapter remains unchanged. This tool locks one of the
four explicitly allowed word-layout interpretations, then converts subsequent
ALSA frames continuously and can feed Gate A-Live through a growing IEC file
or emit a clean binary IEC61937 stream on stdout for a decoder/renderer pipe.
"""
from __future__ import annotations

import argparse
import contextlib
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

from aurora_alsa_iec61937_capture import (
    CHANNELS, FORMAT, FRAME_BYTES, RATE_HZ, CaptureError, canonical_to_s32,
    dump_hw_params, extract_words, score_candidate, synthetic_iec,
)
from aurora_live_ingress import LiveIngressClassifier


class StreamingConverter:
    def __init__(self, *, word_lane="auto", channel_order="auto",
                 lock_bursts=2, max_probe_bytes=16 * 1024 * 1024):
        self.word_lane = word_lane
        self.channel_order = channel_order
        self.lock_bursts = lock_bursts
        self.max_probe_bytes = max_probe_bytes
        self.remainder = bytearray()
        self.probe = bytearray()
        self.selected: tuple[str, str] | None = None
        self.raw_bytes = 0
        self.canonical_bytes = 0
        self.lock_raw_bytes: int | None = None

    def _choices(self):
        lanes = [self.word_lane] if self.word_lane != "auto" else ["high16", "low16"]
        orders = [self.channel_order] if self.channel_order != "auto" else ["lr", "rl"]
        return [(lane, order) for lane in lanes for order in orders]

    def _try_lock(self) -> None:
        scored = []
        data = bytes(self.probe)
        for lane, order in self._choices():
            score, _ = score_candidate(data, word_lane=lane, channel_order=order)
            scored.append(score)
        scored.sort(key=lambda s: (s.consecutive_bursts, s.total_sync_occurrences,
                                   -s.first_sync_offset if s.first_sync_offset >= 0 else -(1 << 62)),
                    reverse=True)
        best = scored[0]
        if best.consecutive_bursts < self.lock_bursts:
            return
        ties = [s for s in scored
                if s.consecutive_bursts == best.consecutive_bursts
                and s.total_sync_occurrences == best.total_sync_occurrences
                and s.first_sync_offset == best.first_sync_offset]
        if len(ties) != 1:
            return
        self.selected = (best.word_lane, best.channel_order)
        self.lock_raw_bytes = self.raw_bytes

    def feed(self, raw: bytes) -> bytes:
        self.raw_bytes += len(raw)
        self.remainder.extend(raw)
        aligned = len(self.remainder) - (len(self.remainder) % FRAME_BYTES)
        if aligned <= 0:
            return b""
        block = bytes(self.remainder[:aligned])
        del self.remainder[:aligned]
        if self.selected is None:
            self.probe.extend(block)
            if len(self.probe) > self.max_probe_bytes:
                raise CaptureError("live converter could not uniquely lock an IEC61937 word layout")
            self._try_lock()
            if self.selected is None:
                return b""
            block = bytes(self.probe)
            self.probe.clear()
        lane, order = self.selected
        canonical = extract_words(block, word_lane=lane, channel_order=order)
        self.canonical_bytes += len(canonical)
        return canonical
    def finish(self) -> None:
        if self.remainder:
            raise CaptureError(f"live raw stream ended with {len(self.remainder)} partial ALSA frame bytes")
        if self.selected is None:
            raise CaptureError("live raw stream ended before IEC61937 word layout could lock")

    def status(self) -> dict:
        return {
            "schema_version": 1,
            "selected_word_lane": self.selected[0] if self.selected else None,
            "selected_channel_order": self.selected[1] if self.selected else None,
            "raw_bytes": self.raw_bytes,
            "canonical_bytes": self.canonical_bytes,
            "lock_raw_bytes": self.lock_raw_bytes,
            "sample_format": FORMAT,
            "channels": CHANNELS,
            "sample_rate_hz": RATE_HZ,
            "truth_boundary": (
                "Continuous ALSA slot conversion only. No physical eARC integrity, hardware reset/drop "
                "counter, decoder, multichannel output or protected-service compatibility is proven."
            ),
        }


def write_status(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
def stream_alsa(*, device: str, iec_out: Path | None, status_out: Path,
                hw_params_log: Path, stderr_log: Path,
                word_lane: str, channel_order: str,
                max_seconds: float | None, chunk_bytes: int,
                status_interval_seconds: float) -> dict:
    arecord = shutil.which("arecord")
    if not arecord:
        raise CaptureError("arecord not found; install alsa-utils on the Linux capture host")
    if chunk_bytes < FRAME_BYTES:
        raise CaptureError("chunk size is too small")
    dump_hw_params(arecord, device, hw_params_log)
    if iec_out is not None:
        iec_out.parent.mkdir(parents=True, exist_ok=True)
    stderr_log.parent.mkdir(parents=True, exist_ok=True)
    command = [arecord, "-D", device, "-f", FORMAT, "-c", str(CHANNELS),
               "-r", str(RATE_HZ), "-t", "raw", "--fatal-errors"]
    converter = StreamingConverter(word_lane=word_lane, channel_order=channel_order)
    started = time.monotonic()
    last_status_write = started - status_interval_seconds
    interrupted = False
    out_context = (
        contextlib.nullcontext(sys.stdout.buffer)
        if iec_out is None
        else iec_out.open("wb")
    )
    with stderr_log.open("wb") as err, out_context as out:
        proc = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=err)
        assert proc.stdout is not None
        try:
            while True:
                if max_seconds is not None and time.monotonic() - started >= max_seconds:
                    break
                chunk = proc.stdout.read(chunk_bytes)
                if not chunk:
                    if proc.poll() is not None:
                        break
                    continue
                canonical = converter.feed(chunk)
                if canonical:
                    out.write(canonical)
                    out.flush()
                now = time.monotonic()
                if now - last_status_write >= status_interval_seconds:
                    write_status(status_out, {**converter.status(),
                        "wall_seconds": now - started,
                        "arecord_running": proc.poll() is None})
                    last_status_write = now
        except KeyboardInterrupt:
            interrupted = True
        finally:
            if proc.poll() is None:
                proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
    converter.finish()
    status = {**converter.status(),
              "wall_seconds": time.monotonic() - started,
              "arecord_exit_code": proc.returncode,
              "operator_interrupted": interrupted}
    write_status(status_out, status)
    if proc.returncode not in (0, -15) and not interrupted:
        raise CaptureError(f"arecord live stream exited with code {proc.returncode}; see {stderr_log}")
    return status
def self_test() -> int:
    canonical = synthetic_iec(4)
    sizes = (3, 4099, 17, 8192, 5, 32768, 11)
    for lane, order in (("high16", "lr"), ("high16", "rl"),
                        ("low16", "lr"), ("low16", "rl")):
        raw = canonical_to_s32(canonical, word_lane=lane, channel_order=order)
        converter = StreamingConverter(lock_bursts=2)
        output = bytearray()
        pos = 0
        index = 0
        while pos < len(raw):
            size = sizes[index % len(sizes)]
            output.extend(converter.feed(raw[pos:pos + size]))
            pos += size
            index += 1
        converter.finish()
        if bytes(output) != canonical:
            raise AssertionError(f"streaming conversion mismatch for {lane}/{order}")
        if converter.selected != (lane, order):
            raise AssertionError(f"layout lock mismatch for {lane}/{order}: {converter.selected}")
        gate = LiveIngressClassifier(lock_bursts=2)
        gate.feed(bytes(output), final=True)
        if gate.state != "LOCKED_JOC" or len(gate.bursts) != 4:
            raise AssertionError("converted stream did not lock Gate A-Live")
    failed = False
    garbage = StreamingConverter(lock_bursts=2, max_probe_bytes=FRAME_BYTES * 128)
    try:
        garbage.feed(bytes(FRAME_BYTES * 129))
    except CaptureError:
        failed = True
    if not failed:
        raise AssertionError("sync-free live stream did not fail closed")
    print("AURORA-ALSA-IEC61937-STREAM-SELFTEST-PASS layouts=4 gate=LOCKED_JOC")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("self-test")
    live = sub.add_parser("capture", help="continuously capture ALSA and append canonical IEC61937")
    live.add_argument("--device", required=True)
    live.add_argument(
        "--iec-out",
        required=True,
        help="canonical IEC61937 output path, or '-' for binary stdout",
    )
    live.add_argument("--status", type=Path, required=True)
    live.add_argument("--hw-params-log", type=Path, required=True)
    live.add_argument("--stderr-log", type=Path, required=True)
    live.add_argument("--word-lane", choices=["auto", "high16", "low16"], default="auto")
    live.add_argument("--channel-order", choices=["auto", "lr", "rl"], default="auto")
    live.add_argument("--max-seconds", type=float)
    live.add_argument(
        "--chunk-bytes",
        type=int,
        default=8192,
        help="raw ALSA read size; 8192 bytes is about 5.3 ms at 192 kHz/S32_LE/stereo",
    )
    live.add_argument(
        "--status-interval-seconds",
        type=float,
        default=0.5,
        help="minimum interval between live status-file rewrites",
    )
    args = parser.parse_args()
    if args.command == "self-test":
        return self_test()
    if args.max_seconds is not None and args.max_seconds <= 0:
        raise CaptureError("--max-seconds must be positive when supplied")
    if args.status_interval_seconds <= 0:
        raise CaptureError("--status-interval-seconds must be positive")
    iec_out = None if args.iec_out == "-" else Path(args.iec_out)
    status = stream_alsa(
        device=args.device,
        iec_out=iec_out,
        status_out=args.status,
        hw_params_log=args.hw_params_log,
        stderr_log=args.stderr_log,
        word_lane=args.word_lane,
        channel_order=args.channel_order,
        max_seconds=args.max_seconds,
        chunk_bytes=args.chunk_bytes,
        status_interval_seconds=args.status_interval_seconds,
    )
    print(
        "AURORA-ALSA-IEC61937-STREAM-PASS "
        f"device={args.device} lane={status['selected_word_lane']} "
        f"order={status['selected_channel_order']} canonical_bytes={status['canonical_bytes']}",
        file=sys.stderr if iec_out is None else sys.stdout,
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except CaptureError as exc:
        print(f"AURORA-ALSA-IEC61937-STREAM-FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
