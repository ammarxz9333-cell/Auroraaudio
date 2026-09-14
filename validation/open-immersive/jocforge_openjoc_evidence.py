#!/usr/bin/env python3
"""Summarize independent OpenJOC acceptance/render evidence for JOCForge vectors."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vector-manifest", type=Path, required=True)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    manifest = json.loads(args.vector_manifest.read_text(encoding="utf-8"))
    vectors = manifest.get("vectors") or []
    if not vectors or manifest.get("vector_count") != len(vectors):
        raise SystemExit("invalid vector manifest")

    results = []
    for vector in vectors:
        vector_id = vector["id"]
        result_dir = args.results_dir / vector_id
        inspect_path = result_dir / "openjoc-inspect.json"
        probe_path = result_dir / "openjoc-7.1.4-ffprobe.json"
        if not inspect_path.is_file() or not probe_path.is_file():
            raise SystemExit(f"missing OpenJOC evidence for {vector_id}")

        inspect = json.loads(inspect_path.read_text(encoding="utf-8"))
        probe = json.loads(probe_path.read_text(encoding="utf-8"))
        joc = inspect.get("joc") or {}
        eac3 = inspect.get("eac3") or {}
        validation = inspect.get("validation") or {}
        diagnostics = inspect.get("diagnostics") or {}
        streams = probe.get("streams") or []
        if not streams:
            raise SystemExit(f"OpenJOC render probe has no stream for {vector_id}")
        stream = streams[0]

        checks = {
            "joc_present": joc.get("present") is True,
            "access_units_positive": int(eac3.get("access_unit_count", 0)) > 0,
            "samples_positive": int(eac3.get("total_samples", 0)) > 0,
            "stream_parse_pass": validation.get("stream_parse") == "pass",
            "decoder_admissible": validation.get("decoder_admissible") is True,
            "diagnostics_complete": diagnostics.get("complete") is True
            and int(diagnostics.get("issue_count", 0)) == 0,
            "render_channels_7_1_4": int(stream.get("channels", 0)) == 12,
            "render_sample_rate_positive": int(stream.get("sample_rate", 0)) > 0,
        }
        if not all(checks.values()):
            failed = sorted(key for key, value in checks.items() if not value)
            raise SystemExit(f"OpenJOC acceptance failed for {vector_id}: {failed}")

        results.append(
            {
                "id": vector_id,
                "source": vector["output"],
                "coverage": vector["coverage"],
                "access_unit_count": int(eac3["access_unit_count"]),
                "total_samples": int(eac3["total_samples"]),
                "reported_profiles": joc.get("profiles") or [],
                "etsi_strict_status": (validation.get("etsi_strict") or {}).get("status"),
                "deployed_compatibility_status": (
                    validation.get("deployed_compatibility") or {}
                ).get("status"),
                "render_channels": int(stream["channels"]),
                "render_sample_rate": int(stream["sample_rate"]),
                "checks": checks,
            }
        )

    summary = {
        "schema_version": 1,
        "reference": {
            "name": "OpenJOC",
            "expected_version": "0.17.0",
            "integration": "independent-external-reference",
        },
        "generator": {
            "name": "JOCForge",
            "commit": manifest["generator_commit"],
        },
        "vector_count": len(results),
        "all_vectors_accepted_and_rendered": len(results) == manifest["vector_count"],
        "results": results,
        "truth_boundary": (
            "This evidence establishes independent OpenJOC 0.17.0 inspection admission and "
            "7.1.4 rendering for the representative JOCForge vector matrix only. It does not "
            "establish Aurora/Harletty equivalence, sample-identical rendering, exhaustive "
            "JOCForge capability coverage, hardware interoperability, protected-service "
            "compatibility, proprietary implementation equivalence, or certification."
        ),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
