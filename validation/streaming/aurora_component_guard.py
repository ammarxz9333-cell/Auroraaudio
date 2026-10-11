#!/usr/bin/env python3
"""Compatibility guard: permit a single Aurora component update only when its
boundary is unchanged. Breaking versions require a paired graph migration.

This validates inert contract metadata. No ability to hot-restart native
Rust drivers is asserted by a passing software compatibility test.
"""
from __future__ import annotations

from copy import deepcopy
import json
from pathlib import Path
import re
from typing import Mapping

CANONICAL_ROLES = (
    "FL", "FR", "FC", "LFE", "SL", "SR", "SBL", "SBR",
    "TFL", "TFR", "TRL", "TRR",
)
EXPECTED_IDS = (
    "earc-ingress", "format-router", "decoder-worker", "spatial-renderer",
    "post-dsp", "speaker-output", "wireless-speaker-fabric",
)
EXPECTED_BOUNDARIES = (
    ("audio-carrier-source/v1", "bounded-encoded-packet/v1"),
    ("bounded-encoded-packet/v1", "verified-decoder-input/v1"),
    ("verified-decoder-input/v1", "decoded-audio-and-objects/v1"),
    ("decoded-audio-and-objects/v1", "aurora-12ch-pcm/v1"),
    ("aurora-12ch-pcm/v1", "aurora-12ch-pcm/v1"),
    ("aurora-12ch-pcm/v1", "device-output/v1"),
    ("device-output/v1", "timed-speaker-output/v1"),
)
EXPECTED_PROTOCOL = [
    "stage-new-generation", "run-component-conformance",
    "validate-adjacent-interfaces", "mute-and-flush",
    "activate-on-media-control-thread",
    "verify-current-generation-only", "rollback-to-last-known-good-on-failure",
]


class ComponentCompatibilityError(ValueError):
    pass


def validate_component_graph(data: Mapping) -> None:
    if data.get("schema_version") != 1:
        raise ComponentCompatibilityError("unknown component contract schema")
    audio = data.get("audio_bus", {})
    if audio != {
        "format": "S32_LE", "sample_rate_hz": 48_000, "channels": 12,
        "role_order": list(CANONICAL_ROLES),
    }:
        raise ComponentCompatibilityError("canonical 7.1.4 bus or order changed")
    modules = data.get("modules", [])
    if [m.get("id") for m in modules] != list(EXPECTED_IDS):
        raise ComponentCompatibilityError("missing/duplicate/reordered audio chain stage")
    for index, module in enumerate(modules):
        if (module.get("requires"), module.get("provides")) != EXPECTED_BOUNDARIES[index]:
            raise ComponentCompatibilityError(
                f"{module['id']}: upstream/downstream API must be migrated explicitly"
            )
        if module.get("independent") is not True:
            raise ComponentCompatibilityError("module cannot share mutable decoder resources")
        if not re.fullmatch(r"[a-z0-9_./-]{8,256}", module.get("implementation", "")):
            raise ComponentCompatibilityError("no stable module ownership path")
        if not module.get("truth"):
            raise ComponentCompatibilityError("each module needs an honest evidence boundary")
    if data.get("required_update_protocol") != EXPECTED_PROTOCOL:
        raise ComponentCompatibilityError("missing rollback / epoch / preactivation safety steps")
    forbidden = data.get("callback_forbidden", [])
    for operation in ("dlopen", "untrusted-plugin-call", "subprocess",
                      "file-io", "network-block", "dynamic-version-switch"):
        if operation not in forbidden:
            raise ComponentCompatibilityError(f"unsafe callback permitted: {operation}")


def independent_update(old: Mapping, new: Mapping) -> tuple[str, ...]:
    """Returns exactly the changed components allowed to upgrade separately."""
    validate_component_graph(old)
    validate_component_graph(new)
    changes = []
    for left, right in zip(old["modules"], new["modules"]):
        if left != right:
            if (
                left["id"] != right["id"]
                or left["requires"] != right["requires"]
                or left["provides"] != right["provides"]
            ):
                raise ComponentCompatibilityError("breaking component ABI requires group upgrade")
            changes.append(left["id"])
    return tuple(changes)


if __name__ == "__main__":
    import sys
    file = Path(sys.argv[1])
    graph = json.loads(file.read_text(encoding="utf-8"))
    validate_component_graph(graph)
    print("AURORA-PIPELINE-COMPONENT-CONTRACT-PASS modules=7 "
          "canonical=12ch/48000/S32LE physical_pass=false")
