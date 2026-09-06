#!/usr/bin/env python3
"""Validate Aurora's external-component decisions and evidence boundaries."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


ALLOWED_DECISIONS = {
    "adopted",
    "adopted-offline",
    "experimental-s6",
    "evaluate",
    "defer-headphones",
    "defer-dialogue",
    "defer-streaming",
    "defer-multiroom",
}


def fail(message: str) -> None:
    raise SystemExit(f"external-components: {message}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "manifest",
        nargs="?",
        type=Path,
        default=Path("config/external-components-v1.json"),
    )
    args = parser.parse_args()

    data = json.loads(args.manifest.read_text(encoding="utf-8"))
    if data.get("schema_version") != 1:
        fail("unsupported schema_version")

    components = data.get("components")
    if not isinstance(components, list) or not components:
        fail("components must be a non-empty list")

    ids: set[str] = set()
    for component in components:
        component_id = component.get("id")
        if not isinstance(component_id, str) or not component_id:
            fail("every component needs a non-empty id")
        if component_id in ids:
            fail(f"duplicate component id: {component_id}")
        ids.add(component_id)

        if component.get("decision") not in ALLOWED_DECISIONS:
            fail(f"{component_id}: unknown decision")
        if not str(component.get("upstream", "")).startswith("https://"):
            fail(f"{component_id}: upstream must be an HTTPS URL")
        if not isinstance(component.get("roles"), list) or not component["roles"]:
            fail(f"{component_id}: roles must be a non-empty list")

        evidence = component.get("evidence")
        if not isinstance(evidence, list):
            fail(f"{component_id}: evidence must be a list")
        if component["decision"] in {"adopted", "adopted-offline", "experimental-s6"}:
            if not component.get("tested_version"):
                fail(f"{component_id}: selected components require a tested_version")
            if not evidence:
                fail(f"{component_id}: selected components require evidence")
        if component.get("production_ready") and not evidence:
            fail(f"{component_id}: production_ready requires evidence")
        if component.get("object_metadata") == "supported" and not evidence:
            fail(f"{component_id}: object metadata support requires evidence")

    required = {"ffmpeg", "camilladsp"}
    if not required.issubset(ids):
        fail(f"missing required selections: {sorted(required - ids)}")

    print(f"external-components: PASS ({len(components)} components)")


if __name__ == "__main__":
    main()
