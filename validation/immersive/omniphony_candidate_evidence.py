#!/usr/bin/env python3
"""Compare a pinned Omniphony evaluation candidate against Aurora's stable lane."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import struct
import sys
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path

CHANNELS = 12
SAMPLE_RATE = 48000
ACTIVE_EPSILON = 1.0e-8
COMPLETION_STATUSES = ("completed", "timeout")


@dataclass
class ChannelMetric:
    index: int
    rms: float
    peak: float
    active: bool


@dataclass
class PcmMetric:
    path: str
    sha256: str
    bytes: int
    channels: int
    sample_rate_hz: int
    frames: int
    duration_seconds: float
    finite: bool
    non_silent: bool
    active_channel_indices: list[int]
    per_channel: list[dict]


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def read_pcm(path: Path) -> PcmMetric:
    data = path.read_bytes()
    frame_bytes = CHANNELS * 4
    if not data or len(data) % frame_bytes:
        raise ValueError(f"{path}: raw-f32 length {len(data)} is not whole {CHANNELS}-channel frames")
    sample_count = len(data) // 4
    values = struct.unpack(f"<{sample_count}f", data)
    finite = all(math.isfinite(value) for value in values)
    if not finite:
        raise ValueError(f"{path}: raw-f32 contains NaN/Inf")
    frames = sample_count // CHANNELS
    per_channel: list[ChannelMetric] = []
    for channel in range(CHANNELS):
        channel_values = values[channel::CHANNELS]
        sum_sq = sum(value * value for value in channel_values)
        rms = math.sqrt(sum_sq / max(1, len(channel_values)))
        peak = max((abs(value) for value in channel_values), default=0.0)
        per_channel.append(ChannelMetric(channel, rms, peak, peak > ACTIVE_EPSILON))
    active = [metric.index for metric in per_channel if metric.active]
    return PcmMetric(
        path=str(path),
        sha256=sha256(path),
        bytes=len(data),
        channels=CHANNELS,
        sample_rate_hz=SAMPLE_RATE,
        frames=frames,
        duration_seconds=frames / SAMPLE_RATE,
        finite=finite,
        non_silent=bool(active),
        active_channel_indices=active,
        per_channel=[asdict(metric) for metric in per_channel],
    )


def canonical_labels(path: Path) -> dict[str, str]:
    text = path.read_text(encoding="utf-8")
    match = re.search(
        r"pub\s+fn\s+canonical_name\s*\([^)]*\).*?match\s+label\s*\{(?P<body>.*?)\n\s*\}\n\}",
        text,
        re.DOTALL,
    )
    if not match:
        raise ValueError(f"cannot locate canonical_name mapping in {path}")
    pairs = dict(re.findall(r"\b([A-Za-z][A-Za-z0-9_]*)\s*=>\s*\"([^\"]+)\"", match.group("body")))
    if not pairs:
        raise ValueError(f"canonical_name mapping is empty in {path}")
    return pairs


def bridge_evidence(path: Path) -> dict[str, int | bool]:
    text = path.read_text(encoding="utf-8", errors="replace")
    match = re.search(
        r"CANDIDATE-BRIDGE-PASS\s+packets=(\d+)\s+frames=(\d+)\s+metadata_frames=(\d+)\s+events=(\d+)\s+object_channels=(\d+)\s+saw_objects=(true|false)",
        text,
    )
    if not match:
        raise ValueError(f"candidate bridge PASS record missing from {path}")
    packets, frames, metadata_frames, events, object_channels = map(int, match.groups()[:5])
    saw_objects = match.group(6) == "true"
    return {
        "packets": packets,
        "frames": frames,
        "metadata_frames": metadata_frames,
        "events": events,
        "object_channels": object_channels,
        "saw_objects": saw_objects,
    }


def latency_lines(path: Path) -> list[str]:
    if not path.is_file():
        return []
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    return [line[:500] for line in lines if "latency" in line.lower()][:40]


def analyze(args: argparse.Namespace) -> int:
    stable = read_pcm(args.stable_pcm)
    candidate = read_pcm(args.candidate_pcm)
    stable_labels = canonical_labels(args.stable_labels)
    candidate_labels = canonical_labels(args.candidate_labels)
    bridge = bridge_evidence(args.bridge_log)

    failures: list[str] = []
    labels_compatible = stable_labels == candidate_labels
    if args.candidate_completion_status == "timeout":
        failures.append("finite_file_completion_timeout")
    if not labels_compatible:
        failures.append("canonical channel-label mapping changed")
    if candidate.channels != 12:
        failures.append(f"candidate channel count is {candidate.channels}, expected 12")
    if not candidate.finite:
        failures.append("candidate PCM is non-finite")
    if not candidate.non_silent:
        failures.append("candidate PCM is silent")
    if candidate.frames != stable.frames:
        failures.append(f"candidate frame count {candidate.frames} != stable {stable.frames}")
    if abs(candidate.duration_seconds - stable.duration_seconds) > (0.5 / SAMPLE_RATE):
        failures.append(
            f"candidate duration {candidate.duration_seconds:.9f}s != stable {stable.duration_seconds:.9f}s"
        )
    if not bridge["saw_objects"]:
        failures.append("candidate bridge API never reported dynamic objects")
    for field in ("packets", "frames", "metadata_frames", "events", "object_channels"):
        if int(bridge[field]) <= 0:
            failures.append(f"candidate bridge evidence has non-positive {field}")

    report = {
        "schema_version": 2,
        "verdict": "pass" if not failures else "reject",
        "stable": {
            "commit": args.stable_commit,
            "pcm": asdict(stable),
            "canonical_labels": stable_labels,
            "latency_diagnostics": latency_lines(args.stable_log),
        },
        "candidate": {
            "commit": args.candidate_commit,
            "completion_status": args.candidate_completion_status,
            "pcm": asdict(candidate),
            "canonical_labels": candidate_labels,
            "latency_diagnostics": latency_lines(args.candidate_log),
            "bridge_evidence": bridge,
        },
        "comparison": {
            "canonical_labels_compatible": labels_compatible,
            "finite_file_completed": args.candidate_completion_status == "completed",
            "frame_count_equal": candidate.frames == stable.frames,
            "duration_delta_seconds": candidate.duration_seconds - stable.duration_seconds,
            "active_channel_count_stable": len(stable.active_channel_indices),
            "active_channel_count_candidate": len(candidate.active_channel_indices),
            "pcm_sha_equal": stable.sha256 == candidate.sha256,
        },
        "failures": failures,
        "truth_boundary": (
            "Evaluation-only external GPL renderer evidence. PASS does not change Aurora's stable Omniphony pin, "
            "does not prove physical hardware, authored-position correctness, Dolby conformance/certification, "
            "or protected streaming-service compatibility. A REJECT means the candidate is not eligible for "
            "promotion under this evaluation contract; it is not an Aurora stable-lane failure."
        ),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        f"AURORA-OMNIPHONY-CANDIDATE-{report['verdict'].upper()} "
        f"completion={args.candidate_completion_status} "
        f"stable_frames={stable.frames} candidate_frames={candidate.frames} "
        f"stable_active={len(stable.active_channel_indices)} candidate_active={len(candidate.active_channel_indices)}"
    )
    if failures:
        for failure in failures:
            print(f"REJECT: {failure}", file=sys.stderr)
    return 0 if not failures else 1


def self_test() -> int:
    def write_pcm(path: Path, frames: int, scale: float) -> None:
        values: list[float] = []
        for frame in range(frames):
            for channel in range(CHANNELS):
                values.append(scale * (1.0 if (frame + channel) % 7 == 0 else 0.1))
        path.write_bytes(struct.pack(f"<{len(values)}f", *values))

    labels = '''pub fn canonical_name(label: RChannelLabel) -> &'static str {\n    use RChannelLabel::*;\n    match label {\n        L => "L",\n        R => "R",\n        Tfl => "TFL",\n        Object => "Object",\n        Unknown => "Unknown",\n    }\n}\n'''
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        stable_pcm = root / "stable.f32"
        candidate_pcm = root / "candidate.f32"
        write_pcm(stable_pcm, 100, 1.0)
        write_pcm(candidate_pcm, 100, 0.9)
        stable_labels = root / "stable-labels.rs"
        candidate_labels = root / "candidate-labels.rs"
        stable_labels.write_text(labels, encoding="utf-8")
        candidate_labels.write_text(labels, encoding="utf-8")
        bridge_log = root / "bridge.log"
        bridge_log.write_text(
            "CANDIDATE-BRIDGE-PASS packets=2 frames=2 metadata_frames=2 events=20 object_channels=2 saw_objects=true\n",
            encoding="utf-8",
        )
        stable_log = root / "stable.log"
        candidate_log = root / "candidate.log"
        stable_log.write_text("renderer latency 5 ms\n", encoding="utf-8")
        candidate_log.write_text("renderer latency 4 ms\n", encoding="utf-8")
        output = root / "report.json"
        ns = argparse.Namespace(
            stable_pcm=stable_pcm,
            candidate_pcm=candidate_pcm,
            stable_labels=stable_labels,
            candidate_labels=candidate_labels,
            bridge_log=bridge_log,
            stable_log=stable_log,
            candidate_log=candidate_log,
            stable_commit="stable",
            candidate_commit="candidate",
            candidate_completion_status="completed",
            output=output,
        )
        if analyze(ns) != 0:
            raise AssertionError("positive candidate self-test rejected")
        payload = json.loads(output.read_text(encoding="utf-8"))
        if payload["verdict"] != "pass" or not payload["comparison"]["canonical_labels_compatible"]:
            raise AssertionError(payload)

        ns.candidate_completion_status = "timeout"
        if analyze(ns) != 1:
            raise AssertionError("finite-file timeout did not reject candidate")
        payload = json.loads(output.read_text(encoding="utf-8"))
        if payload["verdict"] != "reject" or "finite_file_completion_timeout" not in payload["failures"]:
            raise AssertionError(payload)

        ns.candidate_completion_status = "completed"
        changed = labels.replace('Tfl => "TFL"', 'Tfl => "TopFrontLeft"')
        candidate_labels.write_text(changed, encoding="utf-8")
        if analyze(ns) != 1:
            raise AssertionError("canonical label change did not reject candidate")

    print("AURORA-OMNIPHONY-CANDIDATE-EVIDENCE-SELFTEST-PASS")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("self-test")
    analyze_parser = sub.add_parser("analyze")
    analyze_parser.add_argument("--stable-pcm", type=Path, required=True)
    analyze_parser.add_argument("--candidate-pcm", type=Path, required=True)
    analyze_parser.add_argument("--stable-labels", type=Path, required=True)
    analyze_parser.add_argument("--candidate-labels", type=Path, required=True)
    analyze_parser.add_argument("--bridge-log", type=Path, required=True)
    analyze_parser.add_argument("--stable-log", type=Path, required=True)
    analyze_parser.add_argument("--candidate-log", type=Path, required=True)
    analyze_parser.add_argument("--stable-commit", required=True)
    analyze_parser.add_argument("--candidate-commit", required=True)
    analyze_parser.add_argument(
        "--candidate-completion-status",
        choices=COMPLETION_STATUSES,
        default="completed",
        help="whether the candidate renderer self-terminated after finite input",
    )
    analyze_parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "self-test":
        return self_test()
    return analyze(args)


if __name__ == "__main__":
    raise SystemExit(main())
