#!/usr/bin/env python3
"""Generate an Aurora-owned synthetic moving-object DAMF source.

This generator creates only uncompressed synthetic source material: an LFE-only
bed, two deterministic triangle-wave object channels, and explicit time-varying
object positions. It does not encode E-AC-3/JOC and contains no Dolby media or
third-party codec implementation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import struct
import tempfile
from typing import Iterable

SAMPLE_RATE = 48_000
DURATION_SECONDS = 6
TOTAL_FRAMES = SAMPLE_RATE * DURATION_SECONDS
LFE_ID = 3
OBJECT_IDS = (10, 11)
AUDIO_NAME = "aurora-moving.atmos.audio"
METADATA_NAME = "aurora-moving.atmos.metadata"
MANIFEST_NAME = "aurora-moving.atmos"
SUMMARY_NAME = "aurora-moving-source.json"

# The external research encoder used by the optional manual lane reads 24-bit
# big-endian integer CAF essence. Its parser treats flag bit 0 as float and bit
# 1 as little-endian; signed+packed is therefore 0x0c for this fixture.
CAF_LPCM_FLAGS = 0x0C

TRAJECTORIES: dict[int, tuple[tuple[int, tuple[float, float, float]], ...]] = {
    10: (
        (0, (-1.0, 1.0, 0.0)),
        (48_000, (1.0, 1.0, 0.0)),
        (96_000, (1.0, -1.0, 1.0)),
        (144_000, (-1.0, -1.0, 1.0)),
        (192_000, (0.0, 1.0, 1.0)),
        (240_000, (-1.0, 1.0, 0.0)),
    ),
    11: (
        (0, (1.0, -1.0, 0.0)),
        (48_000, (-1.0, -1.0, 0.5)),
        (96_000, (-1.0, 1.0, 1.0)),
        (144_000, (1.0, 1.0, 0.5)),
        (192_000, (0.0, -1.0, 1.0)),
        (240_000, (1.0, -1.0, 0.0)),
    ),
}


def sha256(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as src:
        for chunk in iter(lambda: src.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def triangle_i24(sample_index: int, frequency_hz: int, peak: int) -> int:
    """Return a deterministic integer triangle wave without libm dependency."""
    phase = ((sample_index * frequency_hz * 65_536) // SAMPLE_RATE) & 0xFFFF
    unit = 32_767 - 2 * abs(phase - 32_768)
    value = (unit * peak) // 32_768
    return max(-8_388_608, min(8_388_607, value))


def pack_s24be(value: int) -> bytes:
    if value < 0:
        value += 1 << 24
    return bytes(((value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF))


def write_manifest(path: pathlib.Path) -> None:
    path.write_text(
        """version: 0.5.1
presentations:
  - type: home
    simplified: false
    metadata: aurora-moving.atmos.metadata
    audio: aurora-moving.atmos.audio
    offset: 0.0
    fps: 24
    scBedConfiguration: [3]
    creationTool: aurora-synthetic-moving-damf
    creationToolVersion: 1
    sourceCodec: synthetic
    bedInstances:
      - channels:
          - channel: LFE
            ID: 3
    objects:
      - ID: 10
      - ID: 11
""",
        encoding="utf-8",
    )


def yaml_pos(pos: tuple[float, float, float]) -> str:
    return "[" + ", ".join(f"{value:.3f}" for value in pos) + "]"


def write_metadata(path: pathlib.Path) -> None:
    lines = [
        f"sampleRate: {SAMPLE_RATE}",
        "events:",
        f"  - ID: {LFE_ID}",
        "    samplePos: 0",
        "    active: true",
    ]
    for object_id in OBJECT_IDS:
        for index, (sample_pos, pos) in enumerate(TRAJECTORIES[object_id]):
            lines.extend(
                [
                    f"  - ID: {object_id}",
                    f"    samplePos: {sample_pos}",
                    *( ["    active: true"] if index == 0 else [] ),
                    f"    pos: {yaml_pos(pos)}",
                ]
            )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_caf(path: pathlib.Path) -> None:
    channels = 1 + len(OBJECT_IDS)
    bytes_per_packet = channels * 3
    payload_bytes = TOTAL_FRAMES * bytes_per_packet
    with path.open("wb") as out:
        out.write(b"caff")
        out.write(struct.pack(">HH", 1, 0))

        out.write(b"desc")
        out.write(struct.pack(">q", 32))
        out.write(struct.pack(">d", float(SAMPLE_RATE)))
        out.write(b"lpcm")
        out.write(
            struct.pack(
                ">IIIIII",
                CAF_LPCM_FLAGS,
                bytes_per_packet,
                1,
                channels,
                24,
                0,
            )[:20]
        )

        out.write(b"data")
        out.write(struct.pack(">q", payload_bytes + 4))
        out.write(struct.pack(">I", 0))  # edit count

        for sample_index in range(TOTAL_FRAMES):
            lfe = triangle_i24(sample_index, 55, 80_000)
            obj_10 = triangle_i24(sample_index, 440, 900_000)
            obj_11 = triangle_i24(sample_index, 659, 700_000)
            out.write(pack_s24be(lfe))
            out.write(pack_s24be(obj_10))
            out.write(pack_s24be(obj_11))


def validate_caf(path: pathlib.Path) -> None:
    expected_channels = 1 + len(OBJECT_IDS)
    with path.open("rb") as src:
        if src.read(4) != b"caff":
            raise ValueError("CAF magic mismatch")
        version, flags = struct.unpack(">HH", src.read(4))
        if (version, flags) != (1, 0):
            raise ValueError(f"unexpected CAF header: version={version} flags={flags}")
        if src.read(4) != b"desc":
            raise ValueError("CAF desc chunk missing")
        (desc_size,) = struct.unpack(">q", src.read(8))
        if desc_size != 32:
            raise ValueError(f"unexpected desc size: {desc_size}")
        sample_rate = struct.unpack(">d", src.read(8))[0]
        format_id = src.read(4)
        format_flags, bytes_per_packet, frames_per_packet, channels, bits = struct.unpack(
            ">IIIII", src.read(20)
        )
        if sample_rate != SAMPLE_RATE or format_id != b"lpcm":
            raise ValueError("unexpected CAF LPCM descriptor")
        expected = (CAF_LPCM_FLAGS, expected_channels * 3, 1, expected_channels, 24)
        actual = (format_flags, bytes_per_packet, frames_per_packet, channels, bits)
        if actual != expected:
            raise ValueError(f"unexpected CAF format tuple: {actual!r} != {expected!r}")
        if src.read(4) != b"data":
            raise ValueError("CAF data chunk missing")
        (data_size,) = struct.unpack(">q", src.read(8))
        (edit_count,) = struct.unpack(">I", src.read(4))
        expected_data_size = 4 + TOTAL_FRAMES * expected_channels * 3
        if data_size != expected_data_size or edit_count != 0:
            raise ValueError("unexpected CAF data size/edit count")


def generate(output_dir: pathlib.Path) -> dict[str, object]:
    output_dir.mkdir(parents=True, exist_ok=True)
    manifest = output_dir / MANIFEST_NAME
    metadata = output_dir / METADATA_NAME
    audio = output_dir / AUDIO_NAME
    summary = output_dir / SUMMARY_NAME

    write_manifest(manifest)
    write_metadata(metadata)
    write_caf(audio)
    validate_caf(audio)

    hashes = {
        MANIFEST_NAME: sha256(manifest),
        METADATA_NAME: sha256(metadata),
        AUDIO_NAME: sha256(audio),
    }
    payload: dict[str, object] = {
        "schema": "org.aurora.synthetic-moving-damf-source.v1",
        "sample_rate": SAMPLE_RATE,
        "duration_seconds": DURATION_SECONDS,
        "total_frames": TOTAL_FRAMES,
        "bed": {"id": LFE_ID, "channel": "LFE"},
        "objects": [
            {
                "id": object_id,
                "trajectory": [
                    {"sample_pos": sample_pos, "pos": list(pos)}
                    for sample_pos, pos in TRAJECTORIES[object_id]
                ],
            }
            for object_id in OBJECT_IDS
        ],
        "source_audio": "deterministic integer triangle waves generated by Aurora",
        "files_sha256": hashes,
        "claim_scope": (
            "synthetic DAMF source only; not E-AC-3/JOC evidence and not Dolby-authored, "
            "Dolby-certified, or hardware-conformance evidence"
        ),
    }
    summary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return payload


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="aurora-moving-damf-a-") as a, tempfile.TemporaryDirectory(
        prefix="aurora-moving-damf-b-"
    ) as b:
        first = generate(pathlib.Path(a))
        second = generate(pathlib.Path(b))
        if first["files_sha256"] != second["files_sha256"]:
            raise SystemExit("synthetic DAMF generation is not deterministic")
        for object_id in OBJECT_IDS:
            positions = {pos for _, pos in TRAJECTORIES[object_id]}
            if len(positions) < 4:
                raise SystemExit(f"object {object_id} lacks trajectory diversity")
        print("AURORA-SYNTHETIC-MOVING-DAMF-SELF-TEST-PASS")
        print(json.dumps(first["files_sha256"], indent=2, sort_keys=True))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("output_dir", nargs="?", type=pathlib.Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        if args.output_dir is not None:
            parser.error("output_dir is not used with --self-test")
        self_test()
        return
    if args.output_dir is None:
        parser.error("output_dir is required unless --self-test is used")
    payload = generate(args.output_dir)
    print(json.dumps(payload, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
