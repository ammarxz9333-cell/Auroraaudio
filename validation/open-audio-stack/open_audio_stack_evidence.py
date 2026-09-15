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
    require((args.sof / "src").is_dir(), "SOF source tree missing")
    require((args.libspatialaudio / "include").is_dir(), "libspatialaudio public include tree missing")
    require((args.libspatialaudio / "LICENSE").is_file(), "libspatialaudio license missing")

    rules = config["selection_rules"]
    require(any("one steady-state speaker renderer" in rule for rule in rules), "single-renderer rule missing")
    require(any("adaptive sample-rate correction" in rule for rule in rules), "single-rate-controller rule missing")

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
