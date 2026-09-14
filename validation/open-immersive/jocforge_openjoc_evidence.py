#!/usr/bin/env python3
"""Summarize independent OpenJOC evidence for JOCForge vectors by vector role."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path

FULL_RENDER_REQUIRED = "full-render-required"
INSPECT_CLASSIFICATION_REQUIRED = "inspect-classification-required"
KNOWN_EXPECTATIONS = {FULL_RENDER_REQUIRED, INSPECT_CLASSIFICATION_REQUIRED}


def load_json(path: Path) -> dict:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        raise SystemExit(f"invalid JSON evidence {path}: {exc}") from exc
    if not isinstance(payload, dict) or not payload:
        raise SystemExit(f"empty/non-object JSON evidence: {path}")
    return payload


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vector-manifest", type=Path, required=True)
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    manifest = load_json(args.vector_manifest)
    vectors = manifest.get("vectors") or []
    if not vectors or manifest.get("vector_count") != len(vectors):
        raise SystemExit("invalid vector manifest")

    results = []
    expectation_counts: Counter[str] = Counter()
    structural_parse_counts: Counter[str] = Counter()
    full_render_count = 0

    for vector in vectors:
        vector_id = vector["id"]
        expectation = vector.get("openjoc_expectation")
        if expectation not in KNOWN_EXPECTATIONS:
            raise SystemExit(f"unknown OpenJOC expectation for {vector_id}: {expectation!r}")
        expectation_counts[expectation] += 1

        result_dir = args.results_dir / vector_id
        inspect_path = result_dir / "openjoc-inspect.json"
        if not inspect_path.is_file():
            raise SystemExit(f"missing OpenJOC inspection evidence for {vector_id}")

        inspect = load_json(inspect_path)
        joc = inspect.get("joc") or {}
        eac3 = inspect.get("eac3") or {}
        validation = inspect.get("validation") or {}
        diagnostics = inspect.get("diagnostics") or {}

        common_checks = {
            "joc_present": joc.get("present") is True,
            "access_units_positive": int(eac3.get("access_unit_count", 0)) > 0,
            "samples_positive": int(eac3.get("total_samples", 0)) > 0,
            "validation_contract_present": isinstance(validation, dict) and bool(validation),
            "diagnostics_contract_present": isinstance(diagnostics, dict) and bool(diagnostics),
        }
        if not all(common_checks.values()):
            failed = sorted(key for key, value in common_checks.items() if not value)
            raise SystemExit(f"OpenJOC inspection contract failed for {vector_id}: {failed}")

        base_result = {
            "id": vector_id,
            "source": vector["output"],
            "coverage": vector["coverage"],
            "expectation": expectation,
            "access_unit_count": int(eac3["access_unit_count"]),
            "total_samples": int(eac3["total_samples"]),
            "reported_profiles": joc.get("profiles") or [],
            "stream_parse": validation.get("stream_parse"),
            "decoder_admissible": validation.get("decoder_admissible"),
            "frame_timing_continuity": validation.get("frame_timing_continuity"),
            "metadata_timing_continuity": validation.get("metadata_timing_continuity"),
            "etsi_strict_status": (validation.get("etsi_strict") or {}).get("status"),
            "deployed_compatibility_status": (
                validation.get("deployed_compatibility") or {}
            ).get("status"),
            "diagnostics_complete": diagnostics.get("complete"),
            "diagnostics_issue_count": int(diagnostics.get("issue_count", 0)),
            "common_checks": common_checks,
        }

        if expectation == FULL_RENDER_REQUIRED:
            probe_path = result_dir / "openjoc-7.1.4-ffprobe.json"
            if not probe_path.is_file():
                raise SystemExit(f"missing OpenJOC 7.1.4 render evidence for {vector_id}")
            probe = load_json(probe_path)
            streams = probe.get("streams") or []
            if not streams:
                raise SystemExit(f"OpenJOC render probe has no stream for {vector_id}")
            stream = streams[0]
            checks = {
                "stream_parse_pass": validation.get("stream_parse") == "pass",
                "decoder_admissible": validation.get("decoder_admissible") is True,
                "frame_timing_continuous": validation.get("frame_timing_continuity")
                == "continuous",
                "metadata_timing_continuous": validation.get("metadata_timing_continuity")
                == "continuous",
                "diagnostics_complete": diagnostics.get("complete") is True
                and int(diagnostics.get("issue_count", 0)) == 0,
                "render_channels_7_1_4": int(stream.get("channels", 0)) == 12,
                "render_sample_rate_positive": int(stream.get("sample_rate", 0)) > 0,
            }
            if not all(checks.values()):
                failed = sorted(key for key, value in checks.items() if not value)
                raise SystemExit(f"OpenJOC full-render acceptance failed for {vector_id}: {failed}")
            base_result.update(
                {
                    "classification": "full-render-pass",
                    "render_channels": int(stream["channels"]),
                    "render_sample_rate": int(stream["sample_rate"]),
                    "full_render_checks": checks,
                }
            )
            full_render_count += 1
        else:
            # Minimal conformance probes are intentionally not judged by continuous-programme
            # render gates. Their OpenJOC parser/admission outcome is interoperability evidence.
            stream_parse = validation.get("stream_parse")
            if stream_parse not in {"pass", "fail"}:
                raise SystemExit(
                    f"OpenJOC structural classification missing pass/fail stream_parse for {vector_id}: "
                    f"{stream_parse!r}"
                )
            classification = (
                "inspect-parse-pass"
                if stream_parse == "pass"
                else "inspect-parse-fail"
            )
            structural_parse_counts[classification] += 1
            base_result.update(
                {
                    "classification": classification,
                    "render_required": False,
                    "note": (
                        "This vector is a minimal structural conformance probe. A parse failure is "
                        "recorded as an interoperability finding, not promoted to decoder support and "
                        "not hidden by an allow-failure CI path."
                    ),
                }
            )

        results.append(base_result)

    expected_full = expectation_counts[FULL_RENDER_REQUIRED]
    expected_structural = expectation_counts[INSPECT_CLASSIFICATION_REQUIRED]
    classified_structural = sum(structural_parse_counts.values())
    if full_render_count != expected_full:
        raise SystemExit(
            f"full-render evidence incomplete: expected {expected_full}, got {full_render_count}"
        )
    if classified_structural != expected_structural:
        raise SystemExit(
            "structural OpenJOC classification incomplete: "
            f"expected {expected_structural}, got {classified_structural}"
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
        "full_render_required_count": expected_full,
        "full_render_pass_count": full_render_count,
        "structural_classification_required_count": expected_structural,
        "structural_classified_count": classified_structural,
        "structural_classifications": dict(sorted(structural_parse_counts.items())),
        "all_role_appropriate_evidence_complete": (
            len(results) == manifest["vector_count"]
            and full_render_count == expected_full
            and classified_structural == expected_structural
        ),
        "results": results,
        "truth_boundary": (
            "This evidence proves full OpenJOC 0.17.0 inspection admission and 7.1.4 rendering "
            "only for vectors explicitly marked full-render-required. Minimal JOCForge structural "
            "probes are classified by inspection; parse failures remain recorded interoperability "
            "findings and are not decoder-support claims. This does not establish Aurora/Harletty "
            "renderer equivalence, sample-identical rendering, exhaustive JOCForge coverage, "
            "hardware interoperability, protected-service compatibility, proprietary equivalence, "
            "or certification."
        ),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
