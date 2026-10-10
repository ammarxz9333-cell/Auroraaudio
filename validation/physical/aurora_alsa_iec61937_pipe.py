#!/usr/bin/env python3
"""Pipe live ALSA eARC capture as canonical IEC61937 bytes to stdout.

This is a thin stdout-safe wrapper around aurora_alsa_iec61937_stream.stream_alsa.
Binary audio is written only to stdout; diagnostics and the final summary go to stderr.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from aurora_alsa_iec61937_capture import CaptureError
from aurora_alsa_iec61937_stream import stream_alsa


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--device", required=True)
    parser.add_argument("--status", type=Path, required=True)
    parser.add_argument("--hw-params-log", type=Path, required=True)
    parser.add_argument("--stderr-log", type=Path, required=True)
    parser.add_argument("--word-lane", choices=["auto", "high16", "low16"], default="auto")
    parser.add_argument("--channel-order", choices=["auto", "lr", "rl"], default="auto")
    parser.add_argument("--max-seconds", type=float)
    parser.add_argument("--chunk-bytes", type=int, default=65536)
    args = parser.parse_args()

    if args.max_seconds is not None and args.max_seconds <= 0:
        raise CaptureError("--max-seconds must be positive when supplied")

    status = stream_alsa(
        device=args.device,
        iec_out=Path("/dev/stdout"),
        status_out=args.status,
        hw_params_log=args.hw_params_log,
        stderr_log=args.stderr_log,
        word_lane=args.word_lane,
        channel_order=args.channel_order,
        max_seconds=args.max_seconds,
        chunk_bytes=args.chunk_bytes,
    )
    print(
        "AURORA-ALSA-IEC61937-PIPE-PASS "
        f"device={args.device} lane={status['selected_word_lane']} "
        f"order={status['selected_channel_order']} canonical_bytes={status['canonical_bytes']}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except CaptureError as exc:
        print(f"AURORA-ALSA-IEC61937-PIPE-FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
