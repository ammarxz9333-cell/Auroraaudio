#!/usr/bin/env python3
"""Aurora swappable inexpensive hardware profile checker (offline design gate).

This is NOT a hardware driver, a bill-of-materials quote, an eARC decoder, or a
physical speaker test. It never applies a swap or unmutes an amplifier.
"""
from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Any

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "config/aurora-hardware-modules-v1.json"
ROLES = ("FL", "FR", "FC", "LFE", "SL", "SR", "SBL", "SBR",
         "TFL", "TFR", "TRL", "TRR")
SLOTS = ("compute", "ingress", "output", "amplifier", "wireless")


class HardwareProfileError(ValueError):
    """Reject a false 7.1.4 hardware configuration before any audio is routed."""


@dataclass(frozen=True)
class Assessment:
    choices: dict[str, str]
    compatible_design: bool
    physical_ready: bool
    known_incremental_minimum_eur: float
    missing_cost_slots: tuple[str, ...]
    blockers: tuple[str, ...]
    replacement_procedure: tuple[str, ...]

    def as_dict(self):
        return dict(
            schema_version=1,
            choices=self.choices,
            compatible_design=self.compatible_design,
            physical_ready=self.physical_ready,
            known_incremental_minimum_eur=self.known_incremental_minimum_eur,
            missing_cost_slots=list(self.missing_cost_slots),
            blockers=list(self.blockers),
            replacement_procedure=list(self.replacement_procedure),
            guarantee="CONTRACT CHECK ONLY: NOT A VERIFIED COMPLETE SOUND SYSTEM",
        )


def load_catalog(path: Path = MANIFEST) -> dict:
    data = json.loads(path.read_text(encoding="utf-8"))
    if data.get("schema_version") != 1:
        raise HardwareProfileError("unrecognized hardware catalog schema")
    bus = data.get("canonical_bus", {})
    if (bus.get("channels") != 12 or bus.get("sample_rate_hz") != 48000
            or bus.get("format") != "S32_LE"
            or tuple(bus.get("roles", ())) != ROLES):
        raise HardwareProfileError("source of truth must be the original 12ch Aurora PCM ABI")
    contracts = data.get("module_contracts", {})
    if set(contracts) != set(SLOTS) or set(data.get("options", {})) != set(SLOTS):
        raise HardwareProfileError("module boundary missing or unversioned")
    for slot in SLOTS:
        contract = contracts[slot]
        if not all(isinstance(contract.get(k), str) and "/v" in contract[k]
                   for k in ("requires", "provides")):
            raise HardwareProfileError("unversioned replacement interface")
        ids = [part.get("id") for part in data["options"][slot]]
        if not ids or any(not isinstance(i, str) or not i for i in ids) or len(set(ids)) != len(ids):
            raise HardwareProfileError("invalid / duplicated module option ids")
    if data["selected"].keys() != contracts.keys():
        raise HardwareProfileError("incomplete selected hardware profile")
    if data.get("permitted_closed_external_hardware") != ["lindy-sii9437"]:
        raise HardwareProfileError("only the user's Lindy eARC hardware exception is allowed")
    return data


def _selected(data: Mapping[str, Any], overrides: Mapping[str, str] | None):
    chosen = dict(data["selected"])
    for slot, option_id in (overrides or {}).items():
        if slot not in SLOTS:
            raise HardwareProfileError(f"unknown module role: {slot}")
        chosen[slot] = option_id
    resolved = {}
    for slot in SLOTS:
        matches = [p for p in data["options"][slot] if p["id"] == chosen.get(slot)]
        if len(matches) != 1:
            raise HardwareProfileError(f"unknown {slot} option {chosen.get(slot)!r}")
        resolved[slot] = matches[0]
    return chosen, resolved


def assess(data: Mapping[str, Any], overrides: Mapping[str, str] | None = None) -> Assessment:
    choices, hardware = _selected(data, overrides)
    host, ingress, output, amps, wireless = (hardware[k] for k in SLOTS)
    if host.get("os") != "linux" or not host.get("open_aurora_software"):
        raise HardwareProfileError("Aurora main runtime must be open Linux software")
    if host.get("arch") not in ("aarch64", "x86_64"):
        raise HardwareProfileError("unsupported compute architecture")
    if ingress.get("output") != data["module_contracts"]["ingress"]["provides"]:
        raise HardwareProfileError("eARC receiver does not satisfy Aurora IEC packet interface")
    if ingress.get("hardware_closed") and (
        ingress.get("id") not in data["permitted_closed_external_hardware"]
        or ingress.get("approved_exception") is not True
    ):
        raise HardwareProfileError("unapproved proprietary eARC receiver")
    if output.get("input") != data["module_contracts"]["output"]["requires"]:
        raise HardwareProfileError("output device incompatible with Aurora 12ch PCM")
    if (output.get("analog_outputs", 0) < 12
            or output.get("rate_hz") != 48000
            or output.get("alsa_format") != "S32_LE"):
        raise HardwareProfileError("a 7.1.4 output MUST expose at least 12 coherent S32_LE/48k channels")
    if output.get("shared_clock") is not True:
        raise HardwareProfileError("multiple independent audio clocks cannot count as 7.1.4")
    if not output.get("open_host_driver"):
        raise HardwareProfileError("audio output requires an open Linux driver")
    if amps.get("analog_speaker_amps", 0) < 11 or amps.get("subwoofer_type") != "active":
        raise HardwareProfileError("need eleven independently amplified speaker feeds plus active sub")
    if wireless.get("mode") not in ("wired", "wireless-experimental"):
        raise HardwareProfileError("unknown speaker transport")
    if ingress.get("input") == "file":
        source_blocker = "fixture source is not TV/eARC input"
    else:
        source_blocker = "TV eARC actual codec/stream switching and JOC/MAT not accepted"
    blockers = [source_blocker]
    if not host.get("performance_7_1_4_proven"):
        blockers.append("host decoding/render load and continuous playback not benchmarked")
    if host.get("available_now") is False:
        blockers.append("selected host currently listed as not available from reference retailer")
    if not output.get("firmware_source_confirmed"):
        blockers.append("selected USB/DAC firmware open-source license/source not yet established")
    if not output.get("exact_board_verified"):
        blockers.append("no exact 12+ analog-output DAC hardware part validated")
    if not output.get("physical_12ch_verified"):
        blockers.append("electrical channel mapping, one shared clock, ASRC and 60-min soak unverified")
    if not amps.get("physical_power_verified"):
        blockers.append("11 amplifier feeds, power safety and active sub not physically accepted")
    if wireless["mode"] != "wired" and not wireless.get("synchronization_physically_verified"):
        blockers.append("wireless endpoints require real PTP/clock/dropout acceptance")
    # A missing price means incomplete BOM, not a free component.
    known = 0.0
    missing = []
    for slot, field in (("compute", "reference_price_eur"),
                        ("ingress", "price_incremental_eur"),
                        ("output", "cost_eur"),
                        ("amplifier", "reference_price_eur"),
                        ("wireless", "cost_eur")):
        value = hardware[slot].get(field)
        if value is None:
            missing.append(slot)
        elif isinstance(value, bool) or not isinstance(value, (int, float)) or value < 0:
            raise HardwareProfileError(f"invalid cost for {slot}")
        else:
            known += float(value)
    blockers.append("actual complete 7.1.4 cost unknown until DAC, amplifiers and speakers quoted")
    procedure = (
        "Keep last-known-good profile; stage replacement OFFLINE, do not hot-unplug DAC/amps.",
        "Confirm source/firmware license and matching versioned interface before enabling audio.",
        "Mute speakers, stop decoder/output workers and flush stale PCM generations.",
        "Physically power off for rewiring. Verify one clock and ALSA 12ch/48k/S32_LE.",
        "Test each of 12 output slots electrically, physical polarity, amplifier/sub safety.",
        "Verify codec/format changes, resume playback, 60-min XRUN-free soak and speaker delays.",
        "Only then activate profile; revert to previous generation on failure.",
    )
    return Assessment(choices, True, False, round(known, 2), tuple(missing),
                      tuple(blockers), procedure)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=Path, default=MANIFEST)
    parser.add_argument("--set", action="append", default=[], metavar="MODULE=ID",
                        help="evaluate a substitute without mutating any running hardware")
    parser.add_argument("--require-physical", action="store_true",
                        help="fail until actual device + speaker evidence is accepted")
    args = parser.parse_args()
    try:
        selected = {}
        for item in args.set:
            if item.count("=") != 1:
                raise HardwareProfileError("--set expects MODULE=ID")
            slot, value = item.split("=", 1)
            if slot in selected:
                raise HardwareProfileError("duplicate module override")
            selected[slot] = value
        result = assess(load_catalog(args.catalog), selected)
    except (HardwareProfileError, OSError, KeyError, ValueError) as error:
        print(json.dumps({"compatible_design": False, "error": str(error)}))
        return 2
    print(json.dumps(result.as_dict(), ensure_ascii=False, indent=2))
    return int(args.require_physical and not result.physical_ready)


if __name__ == "__main__":
    raise SystemExit(main())
