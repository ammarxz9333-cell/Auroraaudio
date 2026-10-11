#!/usr/bin/env python3
"""Generate Aurora's flat 16-channel CamillaDSP runtime config.

The generated baseline intentionally applies no room EQ. Omniphony owns the
speaker geometry, LR4 crossover/bass management and baseline headroom. CamillaDSP
provides the realtime 16-channel post-DSP boundary, ALSA output and independent
clock-domain rate matching. A measured room profile may replace this file later.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be > 0")
    return parsed


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--device", required=True)
    p.add_argument("--samplerate", type=positive_int, default=48_000)
    p.add_argument("--channels", type=positive_int, default=16)
    p.add_argument("--chunksize", type=positive_int, default=512)
    p.add_argument("--queuelimit", type=positive_int, default=2)
    p.add_argument("--target-level", type=positive_int, default=512)
    p.add_argument("--adjust-period", type=positive_int, default=3)
    p.add_argument("--playback-format", default="auto")
    args = p.parse_args()

    if args.channels != 16:
        raise SystemExit("Aurora Pi5 profile requires exactly 16 post-render channels")

    playback = {
        "type": "Alsa",
        "channels": args.channels,
        "device": args.device,
    }
    if args.playback_format.lower() != "auto":
        playback["format"] = args.playback_format

    config = {
        "devices": {
            "samplerate": args.samplerate,
            "chunksize": args.chunksize,
            "queuelimit": args.queuelimit,
            "target_level": args.target_level,
            "adjust_period": args.adjust_period,
            "enable_rate_adjust": True,
            "resampler": {
                "type": "AsyncSinc",
                "profile": "Balanced",
            },
            "capture_samplerate": args.samplerate,
            "rate_measure_interval": 1.0,
            "stop_on_rate_change": False,
            "silence_timeout": 0.0,
            "capture": {
                "type": "Stdin",
                "channels": args.channels,
                "format": "F32_LE",
            },
            "playback": playback,
        },
        "filters": {},
        "mixers": {},
        "pipeline": [],
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    # JSON is a valid YAML subset and avoids adding a PyYAML dependency on Pi OS.
    args.output.write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
    print(
        "AURORA-CAMILLADSP-CONFIG-PASS "
        f"device={args.device} channels={args.channels} rate={args.samplerate} "
        f"chunk={args.chunksize} target={args.target_level}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
