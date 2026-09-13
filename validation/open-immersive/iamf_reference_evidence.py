#!/usr/bin/env python3
"""Validate Aurora's rendered IAMF PCM import against direct libiamf output."""

from __future__ import annotations

import json
import math
import struct
import sys
import tempfile
from pathlib import Path


def _u16(data: bytes, offset: int) -> int:
    if offset + 2 > len(data):
        raise ValueError("truncated u16")
    return struct.unpack_from("<H", data, offset)[0]


def _u32(data: bytes, offset: int) -> int:
    if offset + 4 > len(data):
        raise ValueError("truncated u32")
    return struct.unpack_from("<I", data, offset)[0]


def parse_pcm32_wave(path: Path) -> tuple[int, int, bytes, list[float]]:
    data = path.read_bytes()
    if len(data) < 12 or data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError("not RIFF/WAVE")

    fmt = None
    pcm = None
    offset = 12
    while offset + 8 <= len(data):
        chunk_id = data[offset : offset + 4]
        size = _u32(data, offset + 4)
        start = offset + 8
        end = start + size
        if end > len(data):
            raise ValueError("truncated WAVE chunk")
        if chunk_id == b"fmt ":
            if size < 16:
                raise ValueError("short fmt chunk")
            fmt = (
                _u16(data, start),
                _u16(data, start + 2),
                _u32(data, start + 4),
                _u16(data, start + 12),
                _u16(data, start + 14),
            )
        elif chunk_id == b"data":
            pcm = data[start:end]
        offset = end + (size & 1)

    if fmt is None or pcm is None:
        raise ValueError("missing fmt/data chunk")
    encoding, channels, sample_rate, block_align, bits = fmt
    if encoding != 1 or channels != 2 or sample_rate != 48_000 or bits != 32:
        raise ValueError(
            f"unexpected WAVE format encoding={encoding} channels={channels} "
            f"sample_rate={sample_rate} bits={bits}"
        )
    if block_align != channels * 4 or not pcm or len(pcm) % block_align:
        raise ValueError("invalid PCM32 block alignment")

    expected_f32 = bytearray()
    samples: list[float] = []
    for (sample,) in struct.iter_unpack("<i", pcm):
        normalized = sample / 2147483648.0
        packed = struct.pack("<f", normalized)
        expected_f32.extend(packed)
        samples.append(struct.unpack("<f", packed)[0])
    return channels, sample_rate, bytes(expected_f32), samples


def analyze(direct_wav: Path, adapter_f32: Path) -> dict[str, object]:
    channels, sample_rate, expected_f32, samples = parse_pcm32_wave(direct_wav)
    actual_f32 = adapter_f32.read_bytes()
    if len(actual_f32) != len(expected_f32):
        raise ValueError(
            f"adapter byte count {len(actual_f32)} != direct PCM byte count {len(expected_f32)}"
        )
    if len(actual_f32) % 4:
        raise ValueError("adapter F32 output is not sample aligned")

    mismatch_count = sum(
        actual_f32[index : index + 4] != expected_f32[index : index + 4]
        for index in range(0, len(expected_f32), 4)
    )
    if mismatch_count:
        raise ValueError(f"adapter PCM differs from direct libiamf PCM at {mismatch_count} samples")

    decoded = [value[0] for value in struct.iter_unpack("<f", actual_f32)]
    if not decoded or any(not math.isfinite(sample) for sample in decoded):
        raise ValueError("adapter PCM is empty or non-finite")
    peak = max(abs(sample) for sample in decoded)
    rms = math.sqrt(sum(sample * sample for sample in decoded) / len(decoded))
    if rms <= 1e-8:
        raise ValueError("adapter PCM is silent")

    frame_count = len(decoded) // channels
    return {
        "schema_version": 1,
        "verdict": "pass",
        "semantics": "ChannelPcm",
        "sample_rate": sample_rate,
        "channels": channels,
        "frame_count": frame_count,
        "sample_count": len(decoded),
        "sample_bit_mismatches": mismatch_count,
        "peak_abs": peak,
        "rms": rms,
        "objects_claimed": 0,
        "truth_boundary": (
            "Exact PCM agreement proves only that Aurora imports the pinned libiamf iamfdec "
            "rendered stereo PCM without alteration. It does not expose or prove IAMF object "
            "metadata, object-to-PCM bindings, live streaming, elevated/7.1.4 output, HOA "
            "semantics inside Aurora, binaural rendering, physical output, or certification."
        ),
    }


def make_test_wave(path: Path, interleaved: list[int]) -> None:
    pcm = b"".join(struct.pack("<i", value) for value in interleaved)
    header = bytearray()
    header.extend(b"RIFF")
    header.extend(struct.pack("<I", 36 + len(pcm)))
    header.extend(b"WAVEfmt ")
    header.extend(struct.pack("<IHHIIHH", 16, 1, 2, 48_000, 48_000 * 8, 8, 32))
    header.extend(b"data")
    header.extend(struct.pack("<I", len(pcm)))
    path.write_bytes(bytes(header) + pcm)


def self_test() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        direct = root / "direct.wav"
        adapter = root / "adapter.f32le"
        make_test_wave(direct, [0, 1_073_741_824, -1_073_741_824, 536_870_912])
        _, _, expected, _ = parse_pcm32_wave(direct)
        adapter.write_bytes(expected)
        report = analyze(direct, adapter)
        assert report["verdict"] == "pass"
        assert report["sample_bit_mismatches"] == 0

        corrupted = bytearray(expected)
        corrupted[-1] ^= 1
        adapter.write_bytes(corrupted)
        try:
            analyze(direct, adapter)
        except ValueError:
            pass
        else:
            raise AssertionError("corrupted adapter PCM must fail closed")
    print("IAMF-REFERENCE-EVIDENCE-SELF-TEST-PASS")


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "self-test":
        self_test()
        return 0
    if len(sys.argv) != 4:
        print(
            "usage: iamf_reference_evidence.py DIRECT_WAV ADAPTER_F32LE OUTPUT_JSON",
            file=sys.stderr,
        )
        return 2

    direct_wav = Path(sys.argv[1])
    adapter_f32 = Path(sys.argv[2])
    output_json = Path(sys.argv[3])
    try:
        report = analyze(direct_wav, adapter_f32)
    except (OSError, ValueError, struct.error) as error:
        print(f"IAMF-REFERENCE-EVIDENCE-FAIL: {error}", file=sys.stderr)
        return 1

    output_json.parent.mkdir(parents=True, exist_ok=True)
    output_json.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        "IAMF-REFERENCE-EVIDENCE-PASS "
        f"frames={report['frame_count']} samples={report['sample_count']} "
        f"mismatches={report['sample_bit_mismatches']} rms={report['rms']:.9f}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
