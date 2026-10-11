#!/usr/bin/env python3
"""Software-only AC-3/E-AC-3 decoded surround smoke test; no DRM/app proof."""
from __future__ import annotations

import array
import math
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
SCRIPT = ROOT / "aurora_streaming_bed.py"
RATE = 48_000
FREQUENCIES = (431, 557, 673, 71, 887, 997)


def command(argv, **kwargs):
    return subprocess.run(
        argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        check=False, timeout=30, **kwargs
    )


def raw_pcm():
    values = array.array(
        "f", (
            0.12 * math.sin(2 * math.pi * FREQUENCIES[channel] * frame / RATE)
            for frame in range(round(0.4 * RATE))
            for channel in range(6)
        )
    )
    if sys.byteorder != "little":
        values.byteswap()
    return values.tobytes()


def encode(pcm, codec):
    result = command([
        "ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error",
        "-f", "f32le", "-ar", str(RATE), "-ac", "6",
        "-channel_layout", "5.1(side)", "-i", "pipe:0",
        "-c:a", codec, "-b:a", "448k" if codec == "ac3" else "640k",
        "-f", "spdif", "pipe:1",
    ], input=pcm)
    assert result.returncode == 0, result.stderr.decode()
    return result.stdout


def decode(spdif, flags=()):
    return command([sys.executable, str(SCRIPT), *flags], input=spdif)


def parse_pcm(payload):
    assert payload and len(payload) % (12 * 4) == 0
    values = array.array("i")
    values.frombytes(payload)
    if sys.byteorder != "little":
        values.byteswap()
    return values


def rms(values):
    return math.sqrt(sum(value * value for value in values) / len(values))


def verify(codec):
    payload = encode(raw_pcm(), codec)
    result = decode(payload, ("--require-surround-input",))
    assert result.returncode == 0, result.stderr.decode()
    assert f"codec={codec}".encode() in result.stderr
    assert b"source=5.1(side)" in result.stderr
    assert b"objects_decoded=false" in result.stderr
    values = parse_pcm(result.stdout)
    assert len(values) // 12 >= int(0.39 * RATE)

    # Original FL/FR/FC/LFE/SL/SR must stay in fixed Aurora 0..5 slots.
    for channel in range(6):
        decoded = [value / 2**31 for value in values[channel::12]]
        assert rms(decoded) > 0.02, (codec, channel, rms(decoded))
    # Back/height synthetic ambience must not be identical zero for
    # genuinely different left/right spatial material.
    for channel in range(6, 12):
        decoded = [value / 2**31 for value in values[channel::12]]
        assert rms(decoded) > 0.001, (codec, channel, rms(decoded))
    print(f"PASS {codec.upper()} 5.1 -> canonical 12ch 7.1.4 ({len(values)//12} frames)")


def verify_stereo_is_explicit():
    generated = command([
        "ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error",
        "-f", "lavfi", "-i", "sine=frequency=440:duration=0.3",
        "-ac", "2", "-c:a", "ac3", "-f", "spdif", "pipe:1",
    ])
    assert generated.returncode == 0
    reject = decode(generated.stdout, ("--require-surround-input",))
    assert reject.returncode != 0 and b"stereo" in reject.stderr
    upmix = decode(generated.stdout)
    assert upmix.returncode == 0, upmix.stderr.decode()
    assert b"source=stereo" in upmix.stderr
    assert b"objects_decoded=false" in upmix.stderr
    parse_pcm(upmix.stdout)
    print("PASS stereo mode is explicit, never labelled native Atmos or native surround")


def main():
    verify("ac3")
    verify("eac3")
    verify_stereo_is_explicit()
    invalid = decode(b"not an IEC61937 burst")
    assert invalid.returncode != 0 and not invalid.stdout
    print("PASS malformed carrier rejected without releasing PCM")
    print("AURORA-STREAMING-CHANNEL-BED-SOFTWARE-PASS")


if __name__ == "__main__":
    main()
