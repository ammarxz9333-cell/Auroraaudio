#!/usr/bin/env python3
"""Encoded AC-3 <-> E-AC-3 live segment switch integration (real FFmpeg)."""
from __future__ import annotations
import array
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

from test_streaming_bed import encode, raw_pcm

RUNNER = Path(__file__).with_name("aurora_auto_bed.py")
PCM_FRAME = 12 * 4


def run(payload: bytes, *, strict: bool = True):
    with tempfile.TemporaryDirectory(prefix="aurora-auto-bed-") as folder:
        path = Path(folder) / "status.json"
        cmd = [sys.executable, str(RUNNER), "--report", str(path)]
        if strict:
            cmd.append("--strict")
        p = subprocess.run(cmd, input=payload, capture_output=True, timeout=50)
        report = json.loads(path.read_text()) if path.exists() else None
        return p, report


def output_ok(p):
    assert len(p.stdout) > PCM_FRAME * 250, (len(p.stdout), p.stderr[-1600:])
    assert len(p.stdout) % PCM_FRAME == 0
    samples = array.array("i")
    samples.frombytes(p.stdout)
    if sys.byteorder != "little":
        samples.byteswap()
    assert any(samples), "all returned PCM is silent"


def test_two_changes_and_evidence():
    raw = raw_pcm()
    a = encode(raw, "ac3")
    b = encode(raw, "eac3")
    assert a and b
    # Both source formats must be handled as one continuous interleaved
    # canonical PCM stdout without a caller choosing a decoder manually.
    p, report = run(a + b + a)
    assert p.returncode == 0, p.stderr.decode(errors="replace")[-2600:]
    assert report
    counters = report["counters"]
    assert counters["decoder_starts"] == 3, counters
    assert counters["codec_switches"] == 2, counters
    assert counters["valid_ac3_bursts"] >= 10
    assert counters["valid_eac3_bursts"] >= 5
    assert counters["invalid_bursts"] == 0
    assert counters["decoder_failures"] == 0
    assert counters["decoded_pcm_frames"] >= 9600, counters
    assert report["native_atmos_objects"] is False
    output_ok(p)
    print(
        f"PASS AC3 -> DD+ -> AC3 three decoder generations; "
        f"frames={counters['decoded_pcm_frames']}"
    )


def test_bad_carrier_never_reports_success():
    b = encode(raw_pcm(), "ac3")
    assert b[:4] == bytes.fromhex("72f81f4e")
    corrupt = bytearray(b)
    corrupt[30] ^= 0x40  # Usually invalid padding or payload distortion.
    # Set valid-looking Pa/Pb but unknown data type; do not pass it off as
    # DD+ native objects.
    unknown = bytearray(b[:6144])
    unknown[4] = 0x3f
    p, report = run(bytes(unknown) + b)
    assert p.returncode != 0
    assert report["counters"]["invalid_bursts"] >= 1
    assert report["native_atmos_objects"] is False
    print("PASS unknown IEC type fails closed in strict mode")

    p, report = run(b"\xff" * 100_000)
    assert p.returncode != 0
    assert len(p.stdout) == 0
    assert report["counters"]["eof_without_lock"] is True
    print("PASS arbitrary HDMI/non-IEC carrier never becomes fake 7.1.4 PCM")


def test_chunked_hardware_like_capture():
    b = encode(raw_pcm(), "eac3")
    # Burst boundary spans arbitrary ALSA read chunks: same parser must work
    # and decoder cannot depend on input.read returning full packets.
    from aurora_auto_bed import AutoChannelBed
    from io import BytesIO
    out = BytesIO()
    controller = AutoChannelBed(
        output=out, program=Path(__file__).with_name("aurora_streaming_bed.py"),
    )
    for pos in range(0, len(b), 1103):
        controller.feed(b[pos:pos+1103])
    controller.feed(b"", final=True)
    assert controller.stats.decoder_starts == 1
    assert controller.stats.invalid_bursts == 0
    assert controller.stats.decoder_failures == 0
    assert len(out.getvalue()) > PCM_FRAME * 250
    print("PASS arbitrary small capture chunk boundaries")


if __name__ == "__main__":
    test_two_changes_and_evidence()
    test_bad_carrier_never_reports_success()
    test_chunked_hardware_like_capture()
    print("AURORA-AUTO-FORMAT-SWITCH-SOFTWARE-PASS")
