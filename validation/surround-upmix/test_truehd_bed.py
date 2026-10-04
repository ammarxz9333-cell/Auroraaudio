#!/usr/bin/env python3
"""Execute the existing FFmpeg bed/upmix worker on a self-generated TrueHD fixture.

Channel decode and synthetic heights only; no DAMF, Atmos objects or hardware proof.
"""
import array
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys

FFMPEG = os.environ.get("AURORA_FFMPEG_BIN", "ffmpeg")
SHELL = os.environ.get("AURORA_SH_BIN", "sh")
SCRIPT = Path(__file__).with_name("aurora-surround-upmix.sh").resolve()


def run(args, data=None):
    return subprocess.run(args, input=data, capture_output=True, check=True, timeout=30)


def floats(data, channels):
    assert data and len(data) % (channels * 4) == 0, "empty/truncated PCM"
    values = array.array("f")
    values.frombytes(data)
    if sys.byteorder != "little":
        values.byteswap()
    assert all(math.isfinite(x) for x in values), "non-finite PCM"
    return values


def main():
    # Different tones on every channel detect swapped, duplicated or dropped beds.
    source = array.array("i", (
        int(200000 * math.sin(2 * math.pi * frequency * frame / 48000)) * 256
        for frame in range(4800)
        for frequency in (440, 550, 330, 50, 660, 770)
    ))
    if sys.byteorder != "little":
        source.byteswap()
    raw = source.tobytes()
    encoded = run([
        FFMPEG, "-v", "error", "-f", "s32le", "-ar", "48000", "-ac", "6",
        "-channel_layout", "5.1(side)", "-i", "pipe:0", "-c:a", "truehd",
        "-strict", "-2", "-f", "truehd", "pipe:1",
    ], raw).stdout
    decode = [FFMPEG, "-v", "error", "-f", "truehd", "-i", "pipe:0"]
    lossless = run(decode + ["-c:a", "pcm_s32le", "-f", "s32le", "pipe:1"], encoded).stdout
    assert lossless == raw, "TrueHD channels did not round-trip losslessly"
    reference = floats(run(decode + [
        "-ac", "8", "-ar", "48000", "-c:a", "pcm_f32le", "-f", "f32le", "pipe:1",
    ], encoded).stdout, 8)
    command = [SHELL, str(SCRIPT), "--input-format", "truehd"]
    result = run(command, encoded)
    assert b"objects_decoded=false heights=synthetic" in result.stderr
    actual = floats(result.stdout, 12)
    assert len(actual) // 12 == len(reference) // 8 == 4800
    error = max(abs(actual[f * 12 + c] - reference[f * 8 + c])
                for f in range(4800) for c in range(8))
    assert error < 1e-6, f"bed changed: {error}"
    assert all(max(abs(x) for x in actual[c::12]) > 0.001 for c in range(8, 12))
    rejected = subprocess.run(command, input=b"invalid TrueHD", capture_output=True, timeout=30)
    assert rejected.returncode != 0 and not rejected.stdout, "invalid input accepted"
    bad_format = subprocess.run([SHELL, str(SCRIPT), "--input-format", "invented"],
                                input=encoded, capture_output=True, timeout=30)
    assert bad_format.returncode != 0 and not bad_format.stdout
    binary = Path(shutil.which(FFMPEG) or FFMPEG).resolve()
    report = {
        "result": "PASS", "scope": "host-software-generated-5.1-TrueHD-channel-bed",
        "frames": 4800, "lossless_roundtrip": True, "bed_max_error": error,
        "output_channels": 12, "heights": "synthetic", "objects_decoded": False,
        "physical_complete": False, "ffmpeg_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "ffmpeg_version": run([FFMPEG, "-version"]).stdout.decode().splitlines()[0],
        "invalid_input_rejected": True, "unknown_format_rejected": True,
    }
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
