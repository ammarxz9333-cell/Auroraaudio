#!/usr/bin/env python3
"""Bounded evidence for Aurora's external exact-pin EBU libadm validation lane."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tempfile
import xml.etree.ElementTree as ET
from collections import Counter
from pathlib import Path
from typing import Any


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def local_name(name: str) -> str:
    if "}" in name:
        return name.rsplit("}", 1)[1]
    if ":" in name:
        return name.rsplit(":", 1)[1]
    return name


def parse_xml(path: Path) -> ET.Element:
    if not path.is_file() or path.stat().st_size == 0:
        raise ValueError(f"{path}: missing or empty XML")
    return ET.parse(path).getroot()


def element_counts(root: ET.Element) -> Counter[str]:
    return Counter(local_name(element.tag) for element in root.iter())


def find_first(root: ET.Element, wanted: str) -> ET.Element:
    for element in root.iter():
        if local_name(element.tag) == wanted:
            return element
    raise ValueError(f"missing required {wanted} element")


def structural_value(element: ET.Element) -> Any:
    attributes = tuple(sorted((local_name(key), value) for key, value in element.attrib.items()))
    text = (element.text or "").strip()
    children = tuple(structural_value(child) for child in list(element))
    return (local_name(element.tag), attributes, text, children)


def structural_digest(root: ET.Element) -> str:
    afe = find_first(root, "audioFormatExtended")
    encoded = json.dumps(structural_value(afe), ensure_ascii=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def analyze(
    generated_path: Path,
    first_roundtrip_path: Path,
    second_roundtrip_path: Path,
    config_path: Path,
    malformed_exit_code: int,
    aurora_sha: str,
) -> dict[str, Any]:
    config = json.loads(config_path.read_text(encoding="utf-8"))
    validation = config["validation"]
    roundtrip_policy = validation["roundtrip"]
    failures: list[str] = []

    try:
        generated_root = parse_xml(generated_path)
        first_root = parse_xml(first_roundtrip_path)
        second_root = parse_xml(second_roundtrip_path)
    except (ET.ParseError, ValueError) as error:
        return {
            "schema_version": 1,
            "verdict": "fail",
            "failure_reasons": [f"XML evidence parse failed: {error}"],
            "aurora_sha": aurora_sha,
            "libadm_commit": config["reference"]["commit"],
            "truth_boundary": config["truth_boundary"],
        }

    counts_generated = element_counts(generated_root)
    counts_first = element_counts(first_root)
    counts_second = element_counts(second_root)

    minimums = roundtrip_policy["required_minimum_elements"]
    for name, minimum in minimums.items():
        observed = counts_first[name]
        if observed < int(minimum):
            failures.append(f"{name}: observed {observed}, required at least {minimum}")
        if counts_second[name] < int(minimum):
            failures.append(
                f"{name}: second round-trip observed {counts_second[name]}, required at least {minimum}"
            )

    selected = list(roundtrip_policy["stable_element_counts"])
    for name in selected:
        if counts_first[name] != counts_second[name]:
            failures.append(
                f"{name}: round-trip element-count drift {counts_first[name]} -> {counts_second[name]}"
            )

    try:
        digest_first = structural_digest(first_root)
        digest_second = structural_digest(second_root)
    except ValueError as error:
        failures.append(str(error))
        digest_first = None
        digest_second = None
    else:
        if digest_first != digest_second:
            failures.append("audioFormatExtended structural digest changed on second parse/write round-trip")

    if bool(validation["malformed_input"]["must_fail_closed"]) and malformed_exit_code == 0:
        failures.append("malformed ADM XML was accepted instead of failing closed")

    selected_counts = {
        name: {
            "generated": counts_generated[name],
            "roundtrip_1": counts_first[name],
            "roundtrip_2": counts_second[name],
        }
        for name in sorted(set(selected) | set(minimums))
    }

    return {
        "schema_version": 1,
        "verdict": "fail" if failures else "pass",
        "failure_reasons": failures,
        "aurora_sha": aurora_sha,
        "libadm_commit": config["reference"]["commit"],
        "libadm_project_version": config["reference"]["tested_version"],
        "generated_xml_sha256": sha256(generated_path),
        "roundtrip_xml_sha256": sha256(first_roundtrip_path),
        "second_roundtrip_xml_sha256": sha256(second_roundtrip_path),
        "roundtrip_1_structural_sha256": digest_first,
        "roundtrip_2_structural_sha256": digest_second,
        "selected_element_counts": selected_counts,
        "malformed_parse_exit_code": malformed_exit_code,
        "upstream_ctest": "passed-before-evidence-step",
        "truth_boundary": config["truth_boundary"],
    }


def write_self_test_xml(path: Path, object_name: str) -> None:
    path.write_text(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
        "<ebuCoreMain xmlns=\"urn:ebu:metadata-schema:ebuCore_2017\">"
        "<coreMetadata><format><audioFormatExtended>"
        "<audioProgramme audioProgrammeID=\"APR_1001\"/>"
        "<audioContent audioContentID=\"ACO_1001\"/>"
        "<audioContent audioContentID=\"ACO_1002\"/>"
        f"<audioObject audioObjectID=\"AO_1001\"><audioObjectName>{object_name}</audioObjectName></audioObject>"
        "<audioObject audioObjectID=\"AO_1002\"/>"
        "</audioFormatExtended></format></coreMetadata></ebuCoreMain>\n",
        encoding="utf-8",
    )


def self_test() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        generated = root / "generated.xml"
        first = root / "first.xml"
        second = root / "second.xml"
        config = root / "config.json"
        write_self_test_xml(generated, "Alice")
        write_self_test_xml(first, "Alice")
        write_self_test_xml(second, "Alice")
        config.write_text(
            json.dumps(
                {
                    "reference": {"commit": "a" * 40, "tested_version": "test"},
                    "validation": {
                        "roundtrip": {
                            "required_minimum_elements": {
                                "audioProgramme": 1,
                                "audioContent": 2,
                                "audioObject": 2,
                            },
                            "stable_element_counts": [
                                "audioProgramme",
                                "audioContent",
                                "audioObject",
                            ],
                        },
                        "malformed_input": {"must_fail_closed": True},
                    },
                    "truth_boundary": "self-test",
                }
            ),
            encoding="utf-8",
        )
        report = analyze(generated, first, second, config, 2, "b" * 40)
        if report["verdict"] != "pass":
            raise AssertionError(report)

        write_self_test_xml(second, "Bob")
        report = analyze(generated, first, second, config, 2, "b" * 40)
        if report["verdict"] != "fail" or not any("structural digest" in reason for reason in report["failure_reasons"]):
            raise AssertionError("structural-drift self-test did not fail closed")

        write_self_test_xml(second, "Alice")
        report = analyze(generated, first, second, config, 0, "b" * 40)
        if report["verdict"] != "fail" or not any("malformed ADM XML" in reason for reason in report["failure_reasons"]):
            raise AssertionError("malformed-input self-test did not fail closed")

    print("ADM-LIBADM-EVIDENCE-SELF-TEST-PASS")


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "self-test":
        self_test()
        return 0

    parser = argparse.ArgumentParser()
    parser.add_argument("generated_xml", type=Path)
    parser.add_argument("roundtrip_xml", type=Path)
    parser.add_argument("second_roundtrip_xml", type=Path)
    parser.add_argument("config", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--malformed-exit-code", required=True, type=int)
    parser.add_argument("--aurora-sha", default=os.environ.get("GITHUB_SHA", "unknown"))
    args = parser.parse_args()

    report = analyze(
        args.generated_xml,
        args.roundtrip_xml,
        args.second_roundtrip_xml,
        args.config,
        args.malformed_exit_code,
        args.aurora_sha,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if report["verdict"] == "pass":
        print("ADM-LIBADM-REFERENCE-PASS")
        return 0
    print("ADM-LIBADM-REFERENCE-FAIL", file=sys.stderr)
    for reason in report.get("failure_reasons", []):
        print(f"- {reason}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
