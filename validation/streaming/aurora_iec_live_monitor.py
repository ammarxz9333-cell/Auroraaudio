#!/usr/bin/env python3
"""Constant-memory live IEC61937 validation pipe for Aurora's eARC player.

Consumes incoming IEC carrier, validates BEFORE forwarding each chunk, and
writes only bounded diagnostics to disk. No full film-length capture file.
Rejects broken PCM/unknown carriage without feeding it to audio decoders.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "physical"))
from aurora_live_ingress import (  # noqa: E402
    LiveIngressClassifier, VerdictPolicy,
)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--report", required=True, type=Path)
    ap.add_argument("--max-relocks", type=int)
    ap.add_argument("--minimum-valid-bursts", type=int, default=2)
    ap.add_argument("--require-locked-end", action="store_true")
    ap.add_argument("--retained-records", type=int, default=256)
    ap.add_argument("--chunk-bytes", type=int, default=24_576)
    args = ap.parse_args()

    if not 1 <= args.retained_records <= 4096:
        raise ValueError("diagnostics retention must be between 1 and 4096")
    if not 256 <= args.chunk_bytes <= 262_144:
        raise ValueError("bounded IEC ingress read size required")
    policy = VerdictPolicy(
        max_invalid_bursts=0,
        max_unclassified_bytes=0,
        max_relocks=args.max_relocks,
        minimum_valid_bursts=args.minimum_valid_bursts,
        require_locked_end=args.require_locked_end,
    )
    policy.validate()

    classifier = LiveIngressClassifier(record_limit=args.retained_records)
    def forward_validated_burst(frame: bytes):
        # This callback runs ONLY after the whole IEC packet, its syncword,
        # payload length and zero-padding have all been verified.
        # A non-allowed format relock is stopped BEFORE its first burst can
        # reach the decoder.
        if args.max_relocks is not None and classifier.relock_count > args.max_relocks:
            raise ValueError("unexpected codec change: halted before release")
        sys.stdout.buffer.write(frame)
        sys.stdout.buffer.flush()
    processed_chunks = 0
    status = "pass"
    reason = None
    try:
        while True:
            chunk = sys.stdin.buffer.read(args.chunk_bytes)
            if not chunk:
                break
            classifier.feed(chunk, on_valid_burst=forward_validated_burst)
            processed_chunks += 1
            # Bad packets are discarded by the parser itself. Never forward
            # arbitrary bytes from a chunk containing an invalid IEC frame.
            if classifier.invalid_bursts or classifier.unclassified_total:
                status = "fail"
                reason = "invalid-or-unclassified-iec-carrier"
                break
        classifier.feed(b"", final=True, on_valid_burst=forward_validated_burst)
        report = classifier.report(policy)
        if status != "pass":
            report["verdict"] = "fail"
            report["verdict_failures"].append(reason)
        report["runtime_monitor"] = {
            "mode": "constant-memory-fail-closed-passthrough",
            "chunks": processed_chunks,
            "raw_iec_capture_saved": False,
            "max_diagnostic_records": args.retained_records,
        }
    except (ValueError, BrokenPipeError, OSError) as error:
        status = "fail"
        reason = str(error)
        report = classifier.report(policy)
        report["verdict"] = "fail"
        report["verdict_failures"].append(reason)
    finally:
        # The report is always written, including when decoding/pipe output
        # breaks; it never contains sensitive captured media samples.
        if "report" not in locals():
            report = classifier.report(policy)
            report["verdict"] = "fail"
            report["verdict_failures"].append("monitor-runtime-failure")
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(
        "AURORA-IEC-MONITOR " + json.dumps({
            "verdict": report["verdict"],
            "valid_bursts": report["valid_bursts"],
            "invalid_bursts": report["invalid_bursts"],
            "relocks": report["relock_count"],
            "unclassified_bytes": report["unclassified_bytes"],
            "raw_capture_saved": False,
        }, sort_keys=True), file=sys.stderr,
    )
    return 0 if report["verdict"] == "pass" else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, BrokenPipeError) as exc:
        print(f"AURORA-IEC-MONITOR-FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
