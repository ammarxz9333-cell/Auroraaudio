#!/usr/bin/env python3
"""IEC61937 AC-3 / E-AC-3 channel-bed -> Aurora 7.1.4 S32_LE at 48 kHz.

Explicitly does not recover Dolby Atmos JOC or Dolby MAT object positions.
Only receives an authorized encoded PCM carrier from a legitimate eARC tap.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys

PROBE_BYTES = 4 * 24_576
PAN_51_SIDE = (
    "pan=12c|c0=FL|c1=FR|c2=FC|c3=LFE|c4=SL|c5=SR"
    "|c6=0.18*SL-0.18*SR|c7=0.18*SR-0.18*SL"
    "|c8=0.12*FL-0.12*FR|c9=0.12*FR-0.12*FL"
    "|c10=0.12*SL-0.12*SR|c11=0.12*SR-0.12*SL"
)
PAN_51_BACK = (
    "pan=12c|c0=FL|c1=FR|c2=FC|c3=LFE|c4=BL|c5=BR"
    "|c6=0.18*BL-0.18*BR|c7=0.18*BR-0.18*BL"
    "|c8=0.12*FL-0.12*FR|c9=0.12*FR-0.12*FL"
    "|c10=0.12*BL-0.12*BR|c11=0.12*BR-0.12*BL"
)
PAN_71 = (
    "pan=12c|c0=FL|c1=FR|c2=FC|c3=LFE|c4=SL|c5=SR|c6=BL|c7=BR"
    "|c8=0.12*FL-0.12*FR|c9=0.12*FR-0.12*FL"
    "|c10=0.08*SL-0.08*SR+0.08*BL-0.08*BR"
    "|c11=0.08*SR-0.08*SL+0.08*BR-0.08*BL"
)
PAN_STEREO = (
    "pan=12c|c0=0.8*FL|c1=0.8*FR|c2=0.4*FL+0.4*FR|c3=0*FL"
    "|c4=0.16*FL-0.16*FR|c5=0.16*FR-0.16*FL"
    "|c6=0.08*FL-0.08*FR|c7=0.08*FR-0.08*FL"
    "|c8=0.12*FL-0.12*FR|c9=0.12*FR-0.12*FL"
    "|c10=0.06*FL-0.06*FR|c11=0.06*FR-0.06*FL"
)
SUPPORTED = {
    ("stereo", 2): PAN_STEREO,
    ("5.1(side)", 6): PAN_51_SIDE,
    ("5.1", 6): PAN_51_BACK,
    ("7.1", 8): PAN_71,
}


def probe(head: bytes, ffprobe: str) -> tuple[str, int, str]:
    if not head:
        raise ValueError("no IEC61937 input")
    cmd = [
        ffprobe, "-v", "error", "-f", "spdif", "-probesize",
        str(PROBE_BYTES), "-analyzeduration", "200000", "-show_entries",
        "stream=codec_type,codec_name,channels,channel_layout", "-of", "json", "pipe:0",
    ]
    done = subprocess.run(cmd, input=head, capture_output=True, timeout=12, check=False)
    if done.returncode:
        raise ValueError("IEC61937 probe failed (unsupported carrier or truncated bursts)")
    streams = json.loads(done.stdout).get("streams", [])
    audio = [s for s in streams if s.get("codec_type") == "audio"]
    if len(audio) != 1:
        raise ValueError("exactly one identifiable encoded audio stream is required")
    source = audio[0]
    codec = source.get("codec_name", "")
    layout = source.get("channel_layout", "")
    channels = source.get("channels", 0)
    if codec not in ("ac3", "eac3"):
        raise ValueError(f"unsupported IEC61937 codec: {codec}")
    if (layout, channels) not in SUPPORTED:
        raise ValueError(
            f"unsupported decoded layout {layout}/{channels}; no silent channel downmix"
        )
    return codec, channels, layout


def run() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ffmpeg", default=os.environ.get("AURORA_FFMPEG_BIN", "ffmpeg"))
    parser.add_argument("--ffprobe", default=os.environ.get("AURORA_FFPROBE_BIN", "ffprobe"))
    parser.add_argument(
        "--require-surround-input", action="store_true",
        help="reject stereo rather than synthesizing surround",
    )
    args = parser.parse_args()
    if not shutil.which(args.ffmpeg) or not shutil.which(args.ffprobe):
        raise ValueError("ffmpeg and ffprobe are required")

    # Probe only a bounded, prefetched prefix, then replay it into the decoder.
    # No protected-service access or random seeking of the media stream.
    head = sys.stdin.buffer.read(PROBE_BYTES)
    codec, channels, layout = probe(head, args.ffprobe)
    if args.require_surround_input and channels < 6:
        raise ValueError("source is stereo, not native 5.1; explicit surround source required")

    print(
        f"AURORA-STREAMING-BED codec={codec} source={layout} original_channels={channels} "
        "output=7.1.4 output_channels=12 format=S32_LE/48000 "
        "objects_decoded=false synthetic_extra_channels=true",
        file=sys.stderr, flush=True,
    )
    command = [
        args.ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin",
        "-threads", "1", "-probesize", str(PROBE_BYTES),
        "-analyzeduration", "200000", "-f", "spdif", "-i", "pipe:0",
        "-filter_complex_threads", "1", "-af", SUPPORTED[(layout, channels)],
        "-map", "0:a:0", "-ar", "48000", "-c:a", "pcm_s32le",
        "-flush_packets", "1", "-f", "s32le", "pipe:1",
    ]
    decoder = subprocess.Popen(
        command, stdin=subprocess.PIPE, stdout=sys.stdout.buffer, stderr=sys.stderr.buffer
    )
    assert decoder.stdin is not None
    try:
        decoder.stdin.write(head)
        for block in iter(lambda: sys.stdin.buffer.read(65_536), b""):
            decoder.stdin.write(block)
    except BrokenPipeError:
        pass
    finally:
        try:
            decoder.stdin.close()
        except BrokenPipeError:
            pass
    status = decoder.wait()
    if status:
        raise ValueError(f"FFmpeg streaming decode failed with exit status {status}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(run())
    except (ValueError, OSError, subprocess.TimeoutExpired) as error:
        print(f"AURORA-STREAMING-BED-FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
