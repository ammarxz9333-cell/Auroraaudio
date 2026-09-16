#!/usr/bin/env python3
"""Validate exact open-audio-stack source pins and Aurora integration policy."""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path


def git_head(path: Path) -> str:
    return subprocess.check_output(
        ["git", "-C", str(path), "rev-parse", "HEAD"], text=True
    ).strip()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--aoo", required=True, type=Path)
    parser.add_argument("--genavb", required=True, type=Path)
    parser.add_argument("--esp-avb", required=True, type=Path)
    parser.add_argument("--esp-ptp", required=True, type=Path)
    parser.add_argument("--sof", required=True, type=Path)
    parser.add_argument("--libspatialaudio", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    config = json.loads(args.config.read_text())
    require(config.get("schema_version") == 1, "unsupported open-audio-stack schema")

    contract = config["aurora_contract"]
    require(contract["canonical_media_rate_hz"] == 48_000, "canonical media rate drifted")
    require(contract["clock_owner"] == "aurora-media-timeline", "clock ownership drifted")
    require(contract["network_io_in_realtime_callback"] is False, "network I/O entered realtime callback")
    require(contract["double_resampling_allowed"] is False, "double resampling must fail closed")

    paths = {
        "aoo": args.aoo,
        "genavb_tsn": args.genavb,
        "esp_avb": args.esp_avb,
        "esp_ptp": args.esp_ptp,
        "sound_open_firmware": args.sof,
        "libspatialaudio": args.libspatialaudio,
    }
    observed = {}
    for name, path in paths.items():
        expected = config["components"][name]["pinned_commit"]
        actual = git_head(path)
        require(actual == expected, f"{name} pin mismatch: expected {expected}, got {actual}")
        observed[name] = actual

    require((args.aoo / "include" / "aoo.h").is_file(), "AOO public C API header missing")
    require((args.genavb / "api").is_dir(), "GenAVB public API directory missing")
    require((args.genavb / "gptp").is_dir(), "GenAVB gPTP implementation missing")
    require((args.genavb / "avtp").is_dir(), "GenAVB AVTP implementation missing")

    esp_avb_component = (args.esp_avb / "idf_component.yml").read_text()
    require('version: "2.18.0"' in esp_avb_component, "ESP-AVB component version drifted")
    require('license: "MIT"' in esp_avb_component, "ESP-AVB license declaration drifted")
    require('scrambletools/esp_ptp: "*"' in esp_avb_component, "ESP-AVB esp_ptp dependency missing")
    esp_avb_readme = (args.esp_avb / "README.md").read_text()
    require("Up to 2 channels per stream" in esp_avb_readme, "ESP-AVB stereo-per-stream limit missing")
    require("AAF PCM audio, 24 bit, 48 kHz" in esp_avb_readme, "ESP-AVB pinned PCM profile drifted")
    require("ESP32-P4" in esp_avb_readme and "ESP32-C6" in esp_avb_readme, "ESP-AVB target evidence missing")

    esp_ptp_component = (args.esp_ptp / "idf_component.yml").read_text()
    require('version: "1.2.3"' in esp_ptp_component, "ESP-PTP component version drifted")
    require('license: "Apache-2.0"' in esp_ptp_component, "ESP-PTP license declaration drifted")
    esp_ptp_readme = (args.esp_ptp / "README.md").read_text()
    require("IEEE 802.1AS gPTP" in esp_ptp_readme, "ESP-PTP gPTP profile evidence missing")
    require("ESP32-P4" in esp_ptp_readme and "ESP32-C6" in esp_ptp_readme, "ESP-PTP target evidence missing")

    require((args.sof / "src").is_dir(), "SOF source tree missing")
    require((args.libspatialaudio / "include").is_dir(), "libspatialaudio public include tree missing")
    require((args.libspatialaudio / "LICENSE").is_file(), "libspatialaudio license missing")

    rules = config["selection_rules"]
    require(any("one steady-state speaker renderer" in rule for rule in rules), "single-renderer rule missing")
    require(any("adaptive sample-rate correction" in rule for rule in rules), "single-rate-controller rule missing")
    require(any("ESP-AVB plus ESP-PTP" in rule for rule in rules), "ESP endpoint clock-ownership rule missing")
    require(any("stereo-per-stream" in rule for rule in rules), "ESP-AVB scale truth rule missing")

    evidence = {
        "schema_version": 1,
        "status": "source-pins-and-policy-validated",
        "physical_hardware_proven": False,
        "runtime_adapters_selected": False,
        "observed_commits": observed,
        "aurora_contract": contract,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
