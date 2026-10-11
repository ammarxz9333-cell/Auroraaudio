#!/usr/bin/env python3
"""Guard Aurora's curated runtime against duplicated codec/renderer/DSP projects.

The 29-item upstream inventory is a RESEARCH inventory, not an installation
manifest. Only the selected entries in the lean profile may be linked to the
live process graph, and optional format-specialists remain disabled until
independently measured, licensed/authorized where needed, and accepted.
"""
from __future__ import annotations

import json
from pathlib import Path

from aurora_format_routes import REQUIRED_FORMATS

CANONICAL = (
    "FL", "FR", "FC", "LFE", "SL", "SR",
    "SBL", "SBR", "TFL", "TFR", "TRL", "TRR",
)
PRIMARY_IDS = frozenset({
    "ffmpeg-channel-bed", "harletty-omniphony-objects", "camilladsp-room"
})
REQUIRED_ROLES = frozenset({
    "channel-bed", "native-objects", "signal-processing"
})
VALID_SPECIALISTS = frozenset({
    "oxideav-ac4", "fraunhofer-mpeghdec", "libiamf", "not-selected"
})
DISALLOWED_LIVE = frozenset({
    "openjoc", "jocforge", "ittiam-libmpegh", "aomedia-iamf-tools",
    "aomedia-oar", "ebu-ear", "spatial-audio-framework",
    "videolan-libspatialaudio", "google-obr", "ebu-bear",
    "sofar", "roomeq",
})
PAIR = frozenset({"harletty-bridge", "omniphony"})


class ProfileError(ValueError):
    pass


def check_profile(data: dict) -> dict:
    if data.get("schema_version") != 1:
        raise ProfileError("unsupported lean deployment profile schema")
    policy = data.get("policy")
    if not isinstance(policy, dict):
        raise ProfileError("profile policy missing")
    if (policy.get("max_third_party_runtime_groups") != 3
        or policy.get("automatic_upgrade") is not False
        or policy.get("prohibit_external_oracle_loading") is not True
        or policy.get("disallow_parallel_decoder_fallback") is not True
        or policy.get("hot_upgrade_in_audio_callback") is not False
        or policy.get("benchmark_decisions_require_measured_same_host") is not True
        or policy.get("physical_verified") is not False):
        raise ProfileError("unsafe or bloated runtime policy")
    bus = data.get("canonical_output", {})
    if (bus.get("sample_rate_hz") != 48_000 or bus.get("channels") != 12
        or bus.get("format") != "S32_LE"
        or tuple(bus.get("roles", [])) != CANONICAL):
        raise ProfileError("canonical 7.1.4 output shape changed")
    groups = data.get("runtime_groups")
    if not isinstance(groups, list) or not 1 <= len(groups) <= 3:
        raise ProfileError("too many resident external decoder/DSP groups")
    if {g.get("id") for g in groups} != PRIMARY_IDS:
        raise ProfileError("more than one or missing core audio processing group")
    if {g.get("role") for g in groups} != REQUIRED_ROLES:
        raise ProfileError("duplicated decoder or renderer function")
    package_ids = set()
    for group in groups:
        if group.get("runtime") not in {
            "enabled-in-software-ci-only",
            "software-fixture-only",
            "optional-disabled-until-room-eq-configured",
        }:
            raise ProfileError("unsupported deployment readiness claim")
        if not isinstance(group.get("packages"), list) or not group["packages"]:
            raise ProfileError("runtime group requires a pinned package")
        for pkg in group["packages"]:
            if not pkg.get("id") or pkg["id"] in package_ids:
                raise ProfileError("duplicate package installed in two runtime groups")
            package_ids.add(pkg["id"])
            if pkg["id"] in DISALLOWED_LIVE:
                raise ProfileError("differential test oracle cannot run in production audio chain")
            for field in ("previous_pin", "next_pin"):
                if field in pkg:
                    pin = pkg[field]
                    if not isinstance(pin, str) or len(pin) != 40 or any(
                        ch not in "0123456789abcdef" for ch in pin
                    ):
                        raise ProfileError("upstream identity must be immutable full SHA")
    if len(package_ids) != 4:
        raise ProfileError("Aurora reference runtime should require only four third-party packages")
    immersive = next(g for g in groups if g["role"] == "native-objects")
    if {p["id"] for p in immersive["packages"]} != PAIR:
        raise ProfileError("spatial bridge and its renderer must be one linked package group")
    if "bridge_api-0.4" not in immersive.get("abi_pair", ""):
        raise ProfileError("immersion bridge/host version mismatch")
    specials = data.get("conditional_format_specialists", [])
    ids = [p.get("id") for p in specials if p.get("id") != "not-selected"]
    if len(ids) != len(set(ids)) or not set(ids).issubset(VALID_SPECIALISTS):
        raise ProfileError("duplicate or disallowed specialist decoder")
    for entry in specials:
        if entry.get("mode") not in {"DISABLED-BENCH-ONLY", "BLOCKED-UNTIL-TESTED"}:
            raise ProfileError("format specialist enabled without corpus and hardware gates")
        if entry.get("id") in package_ids:
            raise ProfileError("specialist duplicates an active runtime component")
    reference_only = data.get("reference_only", [])
    reference_ids = [ref.get("id") for ref in reference_only]
    if len(reference_ids) != len(set(reference_ids)):
        raise ProfileError("duplicate differential reference")
    if not set(reference_ids).issuperset(DISALLOWED_LIVE):
        raise ProfileError("all excluded research projects must be listed explicitly")
    if set(reference_ids) & package_ids:
        raise ProfileError("research reference accidentally installed as runtime decoder")
    owners = data.get("source_format_owners")
    if not isinstance(owners, dict) or set(owners) != REQUIRED_FORMATS:
        raise ProfileError("some upstream streaming codecs have no single primary owner")
    allowed_owners = PRIMARY_IDS | {x.get("id") for x in specials}
    if not set(owners.values()).issubset(allowed_owners):
        raise ProfileError("codec is routed to unreviewed duplicate decoder")
    for codec, owner in owners.items():
        # No codec may have multiple owners; map-valued owner only.
        if not isinstance(owner, str):
            raise ProfileError(f"{codec}: codec must have exactly one primary")
    for codec in ("eac3-joc", "truehd-atmos", "dts-x"):
        if owners[codec] != immersive["id"]:
            raise ProfileError("native object codec bypasses dedicated paired renderer")
    for codec in ("ac3", "eac3", "dts", "dts-hd", "truehd"):
        if owners[codec] != "ffmpeg-channel-bed":
            raise ProfileError("duplicate ordinary channel-bed codec implementation")
    return {
        "result": "PASS",
        "runtime_groups": len(groups),
        "distinct_upstream_runtime_packages": len(package_ids),
        "single_owner_formats": len(owners),
        "disabled_format_specialists": len(specials),
        "research_only_projects": len(reference_only),
        "physical_audio_verified": False,
        "fastest_backend_claim": "UNMEASURED",
    }


def load_profile(path: Path) -> dict:
    content = json.loads(path.read_text(encoding="utf-8"))
    check_profile(content)
    return content


if __name__ == "__main__":
    import sys
    print(json.dumps(
        check_profile(load_profile(Path(sys.argv[1]))),
        indent=2, sort_keys=True,
    ))
