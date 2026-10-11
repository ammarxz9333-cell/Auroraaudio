#!/usr/bin/env python3
"""Explicit HDMI multichannel LPCM host ingress -> canonical Aurora 7.1.4.

IMPORTANT: receives already-captured raw PCM, NOT Dolby MAT object metadata.
Hardware must supply source channel labels and negotiate HDMI LPCM separately.
"""
from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys

from aurora_streaming_bed import PAN_STEREO, PAN_51_SIDE, PAN_51_BACK, PAN_71

LAYOUTS = {
    "stereo": (2, PAN_STEREO),
    "5.1(side)": (6, PAN_51_SIDE),
    "5.1": (6, PAN_51_BACK),
    "7.1": (8, PAN_71),
}
FORMATS = {"s16le", "s24le", "s32le", "f32le"}
RATES = {32000, 44100, 48000, 88200, 96000, 176400, 192000}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--layout", choices=sorted(LAYOUTS), required=True)
    ap.add_argument("--sample-format", choices=sorted(FORMATS), required=True)
    ap.add_argument("--sample-rate", type=int, required=True)
    ap.add_argument("--require-native-surround", action="store_true")
    ap.add_argument("--ffmpeg", default=os.environ.get("AURORA_FFMPEG_BIN", "ffmpeg"))
    args = ap.parse_args()
    if args.sample_rate not in RATES:
        raise ValueError(f"unsupported explicitly signaled LPCM rate: {args.sample_rate}")
    channels, pan = LAYOUTS[args.layout]
    if args.require_native_surround and channels < 6:
        raise ValueError("stereo source is not native surround")
    if not shutil.which(args.ffmpeg):
        raise ValueError("ffmpeg unavailable")
    # The container accepts channel-labeled PCM only. For MAT the PCM bed
    # alone is insufficient to recover native Atmos object positions.
    print(
        f"AURORA-LPCM-SOURCE channels={channels} layout={args.layout} "
        f"input={args.sample_format}/{args.sample_rate} "
        "output=S32_LE/48000/12ch 7.1.4 native_object_metadata=false "
        "synthetic_missing_channels=true",
        file=sys.stderr, flush=True,
    )
    command = [
        args.ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin", "-threads", "1",
        "-f", args.sample_format, "-ar", str(args.sample_rate), "-ac", str(channels),
        "-channel_layout", args.layout, "-i", "pipe:0",
        "-filter_complex_threads", "1", "-af", pan, "-ar", "48000",
        "-c:a", "pcm_s32le", "-f", "s32le", "pipe:1",
    ]
    decoder = subprocess.Popen(command, stdin=sys.stdin.buffer, stdout=sys.stdout.buffer)
    status = decoder.wait()
    if status:
        raise ValueError(f"PCM channel map failed, ffmpeg exit={status}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as exc:
        print(f"AURORA-LPCM-FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
