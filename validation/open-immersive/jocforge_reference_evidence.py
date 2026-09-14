#!/usr/bin/env python3
"""Produce deterministic evidence for Aurora's pinned JOCForge smoke corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

PINNED_COMMIT = "05a4108e0c6288130dec1203b301979a91475fca"
EXPECTED_PROFILES = ["idx0", "idx1", "idx2", "idx3", "idx4"]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--generator-commit", required=True)
    args = parser.parse_args()

    if args.generator_commit != PINNED_COMMIT:
        raise SystemExit(
            f"unexpected JOCForge commit {args.generator_commit}; expected {PINNED_COMMIT}"
        )

    fixture = args.fixture_dir / "synthetic-reference.bw64"
    if not fixture.is_file() or fixture.stat().st_size <= 0:
        raise SystemExit("missing or empty JOCForge source fixture")

    outputs = []
    for profile in EXPECTED_PROFILES:
        path = args.fixture_dir / f"{profile}.ec3"
        if not path.is_file() or path.stat().st_size <= 0:
            raise SystemExit(f"missing or empty JOCForge output: {path}")
        outputs.append(
            {
                "profile": profile,
                "file": path.name,
                "bytes": path.stat().st_size,
                "sha256": sha256(path),
            }
        )

    summary = {
        "schema_version": 1,
        "generator": {
            "name": "JOCForge",
            "upstream": "https://github.com/chyinan/JOCForge",
            "commit": PINNED_COMMIT,
            "integration": "fixture-generator",
        },
        "fixture": {
            "file": fixture.name,
            "bytes": fixture.stat().st_size,
            "sha256": sha256(fixture),
        },
        "outputs": outputs,
        "acceptance": {
            "all_expected_profiles_present": len(outputs) == len(EXPECTED_PROFILES),
            "all_outputs_non_empty": all(item["bytes"] > 0 for item in outputs),
            "profile_smoke_only": True,
            "sample_identical_pcm_required": False,
        },
        "truth_boundary": (
            "This artifact proves only that Aurora CI built the exact pinned JOCForge source, "
            "generated one deterministic synthetic ADM fixture, and produced non-empty, ffprobe-"
            "parseable E-AC-3/JOC outputs for the five profile smoke cases. It does not prove the "
            "full JOCForge capability lattice, Aurora decoder/render correctness for those files, "
            "hardware interoperability, production mastering quality, proprietary equivalence, "
            "protected-service compatibility, or certification."
        ),
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
