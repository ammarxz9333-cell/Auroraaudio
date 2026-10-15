#!/usr/bin/env python3
"""Single live AC-3 / E-AC-3 channel-bed session with fail-closed format restarts.

Input: authorized canonical IEC61937 stream (pipe from Aurora ALSA eARC).
Output: exact 12-channel interleaved 48-kHz signed S32_LE PCM to stdout.
This intentionally NEVER claims extraction of E-AC-3 JOC objects, Dolby MAT,
TrueHD or DTS:X; the separate native JOC worker remains available.

This is a control-thread/process-side integration, not a realtime callback.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import threading
from dataclasses import dataclass
from pathlib import Path

# One source of truth for IEC burst validation and data type constants.
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "physical"))
from aurora_live_ingress import (  # noqa: E402
    LiveIngressClassifier, PERIOD_BYTES, SYNC, TYPE_AC3, TYPE_EAC3,
)

FRAME_BYTES = 12 * 4
BUFFER_READ = 32 * 1024
MAX_BURST = max(PERIOD_BYTES.values())
# Nonzero gaps and false sync are explicitly faults, not a guessed codec.
ALLOWED_ZERO_GAP = 0


@dataclass
class Statistics:
    valid_ac3_bursts: int = 0
    valid_eac3_bursts: int = 0
    invalid_bursts: int = 0
    byte_gaps: int = 0
    decoder_starts: int = 0
    codec_switches: int = 0
    stop_count: int = 0
    decoded_pcm_frames: int = 0
    dropped_pcm_bytes: int = 0
    decoder_failures: int = 0
    incomplete_bursts: int = 0
    eof_without_lock: bool = False


class Worker:
    """One bounded subprocess. At most one worker can write audible PCM."""

    def __init__(self, epoch: int, mode: int, program: Path, output, state):
        self.epoch = epoch
        self.mode = mode
        self.output = output
        self.state = state
        command = [sys.executable, str(program), "--require-surround-input"]
        self.process = subprocess.Popen(
            command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=sys.stderr,
            bufsize=0,
        )
        self.drain = threading.Thread(target=self._drain, daemon=True)
        self.drain.start()

    def _drain(self):
        assert self.process.stdout is not None
        pending = bytearray()
        try:
            while True:
                fragment = os.read(self.process.stdout.fileno(), 48 * 256)
                if not fragment:
                    break
                pending.extend(fragment)
                length = len(pending) // FRAME_BYTES * FRAME_BYTES
                if length:
                    with self.state["lock"]:
                        if self.state["epoch"] == self.epoch:
                            self.output.write(pending[:length])
                            self.state["stats"].decoded_pcm_frames += length // FRAME_BYTES
                        else:
                            self.state["stats"].dropped_pcm_bytes += length
                    del pending[:length]
        except (BrokenPipeError, OSError, ValueError):
            with self.state["lock"]:
                self.state["stats"].decoder_failures += 1
                self.state["epoch"] += 1
        finally:
            if pending:
                with self.state["lock"]:
                    self.state["stats"].dropped_pcm_bytes += len(pending)
                    self.state["stats"].decoder_failures += 1

    def feed(self, framed_burst: bytes):
        assert self.process.stdin is not None
        try:
            self.process.stdin.write(framed_burst)
        except (BrokenPipeError, OSError) as error:
            raise RuntimeError(f"decoder worker failed on input: {error}") from error

    def stop(self, *, expected_eof: bool):
        # For a clean codec boundary/EOF, drain the entire old valid epoch
        # BEFORE starting the new one. On corruption, invalidate immediately
        # so stale PCM from a failed generation can never become audible.
        if not expected_eof:
            with self.state["lock"]:
                self.state["epoch"] += 1
        assert self.process.stdin is not None
        if expected_eof:
            try:
                self.process.stdin.close()
            except BrokenPipeError:
                pass
            try:
                rc = self.process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
                rc = -1
            # Normal EOF decodes should have already drained. Output is muted
            # after invalidation, so this method is intended for file/CI or
            # a player adapter that tolerates dropping the final small tail.
            if rc != 0:
                self.state["stats"].decoder_failures += 1
        else:
            self.process.terminate()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.drain.join(timeout=3)
        if self.drain.is_alive():
            self.state["stats"].decoder_failures += 1
        # No further frames may pass from the drained old source. The caller
        # can now start a differently typed worker without channel mixing.
        if expected_eof:
            with self.state["lock"]:
                self.state["epoch"] += 1
        self.state["stats"].stop_count += 1


class AutoChannelBed:
    def __init__(self, *, output, program: Path, lock_bursts: int = 2):
        if lock_bursts < 1 or lock_bursts > 16:
            raise ValueError("unsafe lock count")
        self.output = output
        self.program = program
        self.lock_bursts = lock_bursts
        self.buf = bytearray()
        self.state = {"epoch": 0, "lock": threading.Lock(), "stats": Statistics()}
        self.worker: Worker | None = None
        self.current_type: int | None = None
        self.candidate: list[bytes] = []
        self.candidate_type: int | None = None
        self.validator = LiveIngressClassifier(lock_bursts=1)

    @property
    def stats(self) -> Statistics:
        return self.state["stats"]

    def _stop_worker(self, *, eof: bool = False):
        if self.worker:
            self.worker.stop(expected_eof=eof)
            self.worker = None
            self.current_type = None

    def _start(self, kind: int):
        self.state["epoch"] += 1
        self.worker = Worker(self.state["epoch"], kind, self.program, self.output, self.state)
        self.current_type = kind
        self.stats.decoder_starts += 1
        for frame in self.candidate:
            self.worker.feed(frame)
        self.candidate = []
        self.candidate_type = None

    def _fault(self, *, gap_bytes: int = 0, invalid: bool = False):
        self.stats.byte_gaps += gap_bytes
        self.stats.invalid_bursts += int(invalid)
        self._stop_worker()
        self.candidate = []
        self.candidate_type = None

    def _burst(self, frame: bytes, kind: int):
        if kind == TYPE_AC3:
            self.stats.valid_ac3_bursts += 1
        else:
            self.stats.valid_eac3_bursts += 1
        if self.worker and self.current_type == kind:
            self.worker.feed(frame)
            return
        if self.worker:
            self.stats.codec_switches += 1
            self._stop_worker(eof=True)
        if self.candidate_type != kind:
            self.candidate_type = kind
            self.candidate = []
        self.candidate.append(frame)
        if len(self.candidate) >= self.lock_bursts:
            self._start(kind)

    def feed(self, data: bytes, *, final: bool = False):
        self.buf.extend(data)
        while self.buf:
            pos = self.buf.find(SYNC)
            if pos < 0:
                # Retain only the prefix length across chunks. No arbitrary
                # accumulation of unknown HDMI data or LPCM.
                keep = 0 if final else min(3, len(self.buf))
                gap = len(self.buf) - keep
                if gap:
                    self._fault(gap_bytes=gap)
                    del self.buf[:gap]
                break
            if pos:
                self._fault(gap_bytes=pos)
                del self.buf[:pos]
            if len(self.buf) < 8:
                break
            kind = self.buf[4] & 0x7F
            period = PERIOD_BYTES.get(kind)
            if period is None:
                self._fault(invalid=True)
                del self.buf[:4]
                continue
            if len(self.buf) < period:
                break
            frame = bytes(self.buf[:period])
            # Reuse the original Aurora ingress parser: do not create a
            # second, inconsistent AC-3/DD+ checksum/length validator.
            parsed = self.validator._parse_burst(frame, 0)
            if parsed.valid:
                self._burst(frame, kind)
                del self.buf[:period]
            else:
                self._fault(invalid=True)
                del self.buf[:4]  # resynchronize without trusting bad length
        if final:
            if self.buf:
                self.stats.incomplete_bursts += 1
                self._fault(gap_bytes=len(self.buf))
                self.buf.clear()
            if self.worker:
                self._stop_worker(eof=True)
            elif self.stats.decoder_starts == 0:
                self.stats.eof_without_lock = True

    def report(self) -> dict:
        from dataclasses import asdict
        return {
            "schema_version": 1,
            "mode": "automatic-ac3-eac3-channel-bed-only",
            "output": "S32_LE/48000/12ch",
            "native_atmos_objects": False,
            "eac3_joc_objects_potentially_discarded": True,
            "counters": asdict(self.stats),
            "physical_audio_pass": False,
        }


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--lock-bursts", type=int, default=2)
    p.add_argument("--report", type=Path)
    p.add_argument("--strict", action="store_true",
                   help="fail if unknown/corrupt bursts or decoder failures occurred")
    p.add_argument("--source", type=Path,
                   help="optional finite authorized IEC61937 file instead of stdin")
    args = p.parse_args()
    source = args.source.open("rb") if args.source else sys.stdin.buffer
    controller = AutoChannelBed(
        output=sys.stdout.buffer,
        program=Path(__file__).with_name("aurora_streaming_bed.py"),
        lock_bursts=args.lock_bursts,
    )
    try:
        for chunk in iter(lambda: source.read(BUFFER_READ), b""):
            controller.feed(chunk)
        controller.feed(b"", final=True)
        sys.stdout.buffer.flush()
    finally:
        if args.source:
            source.close()
    report = controller.report()
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print("AURORA-AUTO-BED-REPORT " + json.dumps(report, sort_keys=True),
          file=sys.stderr)
    counters = controller.stats
    if counters.decoder_starts == 0 or counters.decoded_pcm_frames == 0:
        return 1
    if args.strict and (
        counters.invalid_bursts or counters.byte_gaps or
        counters.decoder_failures or counters.incomplete_bursts
    ):
        return 1
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError) as exc:
        print(f"AURORA-AUTO-BED-FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
