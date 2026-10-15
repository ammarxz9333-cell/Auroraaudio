#!/usr/bin/env python3
"""Real FFmpeg LPCM 2/5.1/7.1 -> Aurora 12-channel software tests."""
from __future__ import annotations

import array
import math
import subprocess
import sys
from pathlib import Path

SOURCE = Path(__file__).with_name("aurora_lpcm_ingress.py")
FS = 48_000


def pcm(channels: int) -> bytes:
    samples = array.array("f", (
        .10 * math.sin(2 * math.pi * (410 + channel * 137) * frame / FS)
        for frame in range(4_800)
        for channel in range(channels)
    ))
    if sys.byteorder != "little":
        samples.byteswap()
    return samples.tobytes()


def test_layout(name: str, channels: int):
    cmd = [
        sys.executable, str(SOURCE), "--layout", name,
        "--sample-format", "f32le", "--sample-rate", "48000"
    ]
    result = subprocess.run(cmd, input=pcm(channels), capture_output=True, timeout=20)
    assert result.returncode == 0, (name, result.stderr.decode())
    assert b"native_object_metadata=false" in result.stderr
    assert b"output=S32_LE/48000/12ch" in result.stderr
    assert len(result.stdout) % 48 == 0
    output = array.array("i")
    output.frombytes(result.stdout)
    if sys.byteorder != "little":
        output.byteswap()
    assert len(output) // 12 == 4_800
    for channel in (0, 1):
        assert max(abs(v) for v in output[channel::12]) > 100_000
    print(f"PASS LPCM {name} ({channels} native channels) -> 7.1.4 (12 PCM slots)")


def test_rejection():
    cmd = [
        sys.executable, str(SOURCE), "--layout", "stereo",
        "--sample-format", "f32le", "--sample-rate", "48000",
        "--require-native-surround"
    ]
    result = subprocess.run(cmd, input=pcm(2), capture_output=True, timeout=20)
    assert result.returncode != 0
    assert b"stereo" in result.stderr
    assert not result.stdout
    print("PASS stereo strict-mode failure without a fake native surround claim")


if __name__ == "__main__":
    for layout, count in (("stereo", 2), ("5.1(side)", 6), ("5.1", 6), ("7.1", 8)):
        test_layout(layout, count)
    test_rejection()
    print("AURORA-LPCM-SOFTWARE-PASS")
