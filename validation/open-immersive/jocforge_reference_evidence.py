#!/usr/bin/env python3
"""Produce deterministic evidence for Aurora's pinned JOCForge representative corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

PINNED_COMMIT = "05a4108e0c6288130dec1203b301979a91475fca"
REQUIRED_COVERAGE = {
    "idx0",
    "idx1",
    "idx2",
    "idx3",
    "idx4",
    "partition-6",
    "partition-3+3",
    "partition-2+2+2",
    "partition-1x6",
    "multiple-dependent-substreams",
    "independent-lfe",
    "dependent-lfe",
    "deployed-emdf",
    "strict-emdf",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture-dir", type=Path, required=True)
    parser.add_argument("--vector-manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--generator-commit", required=True)
    args = parser.parse_args()

    if args.generator_commit != PINNED_COMMIT:
        raise SystemExit(
            f"unexpected JOCForge commit {args.generator_commit}; expected {PINNED_COMMIT}"
        )

    manifest = json.loads(args.vector_manifest.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != 1:
        raise SystemExit("unsupported JOCForge vector manifest schema")
    if manifest.get("generator_commit") != PINNED_COMMIT:
        raise SystemExit("vector manifest generator commit does not match pinned JOCForge commit")

    vectors = manifest.get("vectors")
    if not isinstance(vectors, list) or not vectors:
        raise SystemExit("vector manifest must contain a non-empty vectors list")
    if manifest.get("vector_count") != len(vectors):
        raise SystemExit("vector manifest count does not match vectors list")

    fixture = args.fixture_dir / "synthetic-reference.bw64"
    if not fixture.is_file() or fixture.stat().st_size <= 0:
        raise SystemExit("missing or empty JOCForge source fixture")

    ids: set[str] = set()
    files: set[str] = set()
    coverage: set[str] = set()
    outputs = []
    for vector in vectors:
        vector_id = vector.get("id")
        output_name = vector.get("output")
        vector_coverage = vector.get("coverage")
        if not isinstance(vector_id, str) or not vector_id or vector_id in ids:
            raise SystemExit(f"invalid or duplicate vector id: {vector_id!r}")
        if not isinstance(output_name, str) or not output_name or output_name in files:
            raise SystemExit(f"invalid or duplicate vector output: {output_name!r}")
        if not isinstance(vector_coverage, list) or not vector_coverage:
            raise SystemExit(f"vector {vector_id} needs non-empty coverage")

        path = args.fixture_dir / output_name
        if not path.is_file() or path.stat().st_size <= 0:
            raise SystemExit(f"missing or empty JOCForge output: {path}")

        ids.add(vector_id)
        files.add(output_name)
        coverage.update(str(item) for item in vector_coverage)
        outputs.append(
            {
                "id": vector_id,
                "file": output_name,
                "coverage": vector_coverage,
                "bytes": path.stat().st_size,
                "sha256": sha256(path),
            }
        )

    missing_coverage = sorted(REQUIRED_COVERAGE - coverage)
    if missing_coverage:
        raise SystemExit(f"representative matrix is missing required coverage: {missing_coverage}")

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
        "matrix": {
            "vector_count": len(outputs),
            "required_coverage_satisfied": True,
            "coverage": sorted(coverage),
        },
        "outputs": outputs,
        "acceptance": {
            "all_manifest_vectors_present": len(outputs) == manifest["vector_count"],
            "all_outputs_non_empty": all(item["bytes"] > 0 for item in outputs),
            "representative_matrix_only": True,
            "sample_identical_pcm_required": False,
        },
        "truth_boundary": (
            "This artifact proves only that Aurora CI built the exact pinned JOCForge source, "
            "generated one deterministic synthetic ADM fixture, and produced the committed "
            "representative structural vector matrix as non-empty files with deterministic hashes. "
            "The workflow separately checks ffprobe parseability. It does not prove the full "
            "JOCForge capability lattice, Aurora decoder/render correctness for these files, "
            "hardware interoperability, production mastering quality, proprietary equivalence, "
            "protected-service compatibility, or certification."
        ),
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
