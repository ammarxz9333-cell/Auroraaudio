#!/usr/bin/env python3
"""Expose exact-pin ESP-PTP clock-stability evidence for Aurora validation.

The pinned esp_ptp API exposes whether a remote source is selected, but that is
not the same thing as the daemon's own servo-stability decision. This narrow
validation patch persists the existing internal "clock is stabilized" result in
ptp_state_s and copies it into ptpd_status_s. It does not change the stability
algorithm or production timing behaviour.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

PINNED_ESP_PTP_COMMIT = "5b7eec233a93733ae954beefb6df3bb9c12dc901"

HEADER_OLD = """  /* Is there a valid remote clock source active? */\n\n  bool clock_source_valid;\n\n  /* Information about selected best clock source */\n"""

HEADER_NEW = """  /* Is there a valid remote clock source active? */\n\n  bool clock_source_valid;\n\n  /* Aurora physical-validation evidence: true only after the daemon's\n   * existing servo-stability gate reports \"clock is stabilized\" for the\n   * currently selected source. This field is added only to the exact-pinned\n   * validation build and does not alter the stability algorithm. */\n  bool clock_stable;\n\n  /* Information about selected best clock source */\n"""

STATE_OLD = """  bool selected_source_valid;            /* True if operating as client */\n  struct ptp_announce_s selected_source; /* Currently selected server */\n"""

STATE_NEW = """  bool selected_source_valid;            /* True if operating as client */\n  bool clock_stable;                    /* Existing servo stability result */\n  struct ptp_announce_s selected_source; /* Currently selected server */\n"""

RESET_OLD = """  state->selected_source_valid = false;\n  memset(&state->selected_source, 0, sizeof(state->selected_source));\n"""

RESET_NEW = """  state->selected_source_valid = false;\n  state->clock_stable = false;\n  memset(&state->selected_source, 0, sizeof(state->selected_source));\n"""

SWITCH_OLD = """      state->selected_source = *msg;\n      state->port[0].last_received_sync = state->port[0].last_received_announce;\n"""

SWITCH_NEW = """      state->clock_stable = false;\n      state->selected_source = *msg;\n      state->port[0].last_received_sync = state->port[0].last_received_announce;\n"""

STABLE_OLD = """  if (cnt > 3) {\n    ptpdebug(\"clock is stabilized\");\n    state->port[0].can_send_delayreq = true;\n  } else {\n    ptpdebug(\"clock is still unstable\");\n  }\n  state->last_offset_ns = offset_ns;\n"""

STABLE_NEW = """  state->clock_stable = cnt > 3;\n  if (state->clock_stable) {\n    ptpdebug(\"clock is stabilized\");\n    state->port[0].can_send_delayreq = true;\n  } else {\n    ptpdebug(\"clock is still unstable\");\n  }\n  state->last_offset_ns = offset_ns;\n"""

STATUS_OLD = """  status->ptp_profile = state->active_ptp_profile;\n  status->peer_is_endpoint = state->port[0].peer_is_endpoint;\n  status->clock_source_valid = state->selected_source_valid;\n\n  /* Copy own identity info to status struct */\n"""

STATUS_NEW = """  status->ptp_profile = state->active_ptp_profile;\n  status->peer_is_endpoint = state->port[0].peer_is_endpoint;\n  status->clock_source_valid = state->selected_source_valid;\n  status->clock_stable = state->selected_source_valid && state->clock_stable;\n\n  /* Copy own identity info to status struct */\n"""


def replace_exact(path: Path, old: str, new: str, label: str) -> None:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise ValueError(f"cannot read {path}: {exc}") from exc
    count = text.count(old)
    if count != 1:
        raise ValueError(f"expected exactly one {label} anchor in {path}, found {count}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def apply(root: Path) -> None:
    header = root / "include" / "esp_ptp.h"
    source = root / "ptp.c"
    if not header.is_file() or not source.is_file():
        raise ValueError("root does not look like an esp_ptp checkout")
    replace_exact(header, HEADER_OLD, HEADER_NEW, "public-status")
    replace_exact(source, STATE_OLD, STATE_NEW, "daemon-state")
    replace_exact(source, RESET_OLD, RESET_NEW, "profile-reset")
    replace_exact(source, SWITCH_OLD, SWITCH_NEW, "source-switch")
    replace_exact(source, STABLE_OLD, STABLE_NEW, "servo-stability")
    replace_exact(source, STATUS_OLD, STATUS_NEW, "status-copy")


def check(root: Path) -> None:
    header = (root / "include" / "esp_ptp.h").read_text(encoding="utf-8")
    source = (root / "ptp.c").read_text(encoding="utf-8")
    required_header = ("bool clock_source_valid;", "bool clock_stable;")
    required_source = (
        "bool clock_stable;                    /* Existing servo stability result */",
        "state->clock_stable = false;",
        "state->clock_stable = cnt > 3;",
        "status->clock_stable = state->selected_source_valid && state->clock_stable;",
    )
    missing = [token for token in required_header if token not in header]
    missing += [token for token in required_source if token not in source]
    if missing:
        raise ValueError("lock instrumentation check missing: " + ", ".join(missing))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("apply", "check", "apply-check"))
    parser.add_argument("--root", type=Path, required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        if args.command in ("apply", "apply-check"):
            apply(args.root)
        if args.command in ("check", "apply-check"):
            check(args.root)
    except (OSError, ValueError) as exc:
        print(f"aurora-esp-ptp-lock-instrumentation: FAIL: {exc}", file=sys.stderr)
        return 1
    print(
        "aurora-esp-ptp-lock-instrumentation: PASS "
        f"pin={PINNED_ESP_PTP_COMMIT} command={args.command}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
