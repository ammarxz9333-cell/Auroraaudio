#!/usr/bin/env python3
"""Expose Aurora detachable endpoint scheduler telemetry through avb_status().

Apply this only after the listener-evidence, AEM endpoint, and PTP-deadline
patches. It records control-plane scheduler timing inside the serialized
esp_avb status path so physical trials can correlate firmware behavior with an
independent electrical measurement.

The recorded scheduler timestamp is NOT an electrical-output timestamp and must
never be used by itself as physical acceptance evidence.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

PINNED_ESP_AVB_COMMIT = "5e75bd3ed91b5407a254a5e49bfc18fc35e6cbb9"

CONTROL_TAIL_OLD = """  uint64_t armed_start_ptp_ns;
  aurora_endpoint_phase_t phase;
  bool muted;
} aurora_endpoint_control_s;"""

CONTROL_TAIL_NEW = """  uint64_t armed_start_ptp_ns;
  aurora_endpoint_phase_t phase;
  bool muted;

  /* Aurora scheduler telemetry. This records firmware/control-plane timing,
   * not measured electrical amplifier output. */
  uint64_t scheduler_unmute_ptp_ns;
  uint32_t scheduler_unmute_count;
  uint32_t deadline_fault_count;
} aurora_endpoint_control_s;"""

PUBLIC_LISTENER_TAIL_OLD = """  } listener_evidence;

  struct {
    uint8_t id[8]; // Entity ID
  } entity;
"""

PUBLIC_LISTENER_TAIL_NEW = """  } listener_evidence;

  /* Aurora detachable endpoint control/scheduler evidence. These fields are
   * copied through avb_status() under the component's existing serialized
   * request path. scheduler_unmute_ptp_ns is firmware timing only. */
  struct {
    bool present;
    uint64_t active_epoch;
    uint8_t active_role0;
    uint8_t active_role1;
    uint64_t requested_start_ptp_ns;
    uint64_t scheduler_unmute_ptp_ns;
    uint32_t scheduler_unmute_count;
    uint32_t deadline_fault_count;
    uint8_t phase;
    bool muted;
  } aurora_endpoint_evidence;

  struct {
    uint8_t id[8]; // Entity ID
  } entity;
"""

STATUS_LISTENER_TAIL_OLD = """    if (valid.frames_rx) {
      status->listener_evidence.frames_rx =
          (uint32_t)octets_to_uint(counters.frames_rx, 4);
    }
  }
"""

STATUS_LISTENER_TAIL_NEW = """    if (valid.frames_rx) {
      status->listener_evidence.frames_rx =
          (uint32_t)octets_to_uint(counters.frames_rx, 4);
    }
  }

  status->aurora_endpoint_evidence.present = true;
  status->aurora_endpoint_evidence.active_epoch =
      state->aurora_endpoint.active_epoch;
  status->aurora_endpoint_evidence.active_role0 =
      state->aurora_endpoint.active_role0;
  status->aurora_endpoint_evidence.active_role1 =
      state->aurora_endpoint.active_role1;
  status->aurora_endpoint_evidence.requested_start_ptp_ns =
      state->aurora_endpoint.armed_start_ptp_ns;
  status->aurora_endpoint_evidence.scheduler_unmute_ptp_ns =
      state->aurora_endpoint.scheduler_unmute_ptp_ns;
  status->aurora_endpoint_evidence.scheduler_unmute_count =
      state->aurora_endpoint.scheduler_unmute_count;
  status->aurora_endpoint_evidence.deadline_fault_count =
      state->aurora_endpoint.deadline_fault_count;
  status->aurora_endpoint_evidence.phase =
      (uint8_t)state->aurora_endpoint.phase;
  status->aurora_endpoint_evidence.muted =
      state->aurora_endpoint.muted;
"""

DEADLINE_FAULT_OLD = """static void aurora_endpoint_deadline_fault(avb_state_s *state,
                                           const char *reason) {
  aurora_endpoint_cancel_deadline();
  state->aurora_endpoint.phase = aurora_endpoint_fault;
  state->aurora_endpoint.muted = true;
  avbwarn("Aurora endpoint deadline fail-closed: %s", reason);
}"""

DEADLINE_FAULT_NEW = """static void aurora_endpoint_deadline_fault(avb_state_s *state,
                                           const char *reason) {
  aurora_endpoint_cancel_deadline();
  state->aurora_endpoint.phase = aurora_endpoint_fault;
  state->aurora_endpoint.muted = true;
  state->aurora_endpoint.deadline_fault_count++;
  avbwarn("Aurora endpoint deadline fail-closed: %s", reason);
}"""

UNMUTE_OLD = """  ctrl->muted = false;
  aurora_endpoint_set_output_muted(false);
  s_aurora_unmute_state = NULL;
  s_aurora_unmute_epoch = 0;
}"""

UNMUTE_NEW = """  ctrl->muted = false;
  ctrl->scheduler_unmute_ptp_ns = now_ns;
  ctrl->scheduler_unmute_count++;
  aurora_endpoint_set_output_muted(false);
  s_aurora_unmute_state = NULL;
  s_aurora_unmute_epoch = 0;
}"""

SCHEDULE_OLD = """  s_aurora_unmute_state = state;
  s_aurora_unmute_epoch = ctrl->active_epoch;
  aurora_endpoint_set_output_muted(true);
"""

SCHEDULE_NEW = """  ctrl->scheduler_unmute_ptp_ns = 0;
  s_aurora_unmute_state = state;
  s_aurora_unmute_epoch = ctrl->active_epoch;
  aurora_endpoint_set_output_muted(true);
"""

MUTE_RESET_OLD = """    ctrl->active_role0 = AURORA_ENDPOINT_ROLE_UNUSED;
    ctrl->active_role1 = AURORA_ENDPOINT_ROLE_UNUSED;
    ctrl->armed_start_ptp_ns = 0;
    ctrl->phase = aurora_endpoint_muted;"""

MUTE_RESET_NEW = """    ctrl->active_role0 = AURORA_ENDPOINT_ROLE_UNUSED;
    ctrl->active_role1 = AURORA_ENDPOINT_ROLE_UNUSED;
    ctrl->armed_start_ptp_ns = 0;
    ctrl->scheduler_unmute_ptp_ns = 0;
    ctrl->phase = aurora_endpoint_muted;"""


def replace_once(path: Path, old: str, new: str, label: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise ValueError(f"expected exactly one {label} anchor in {path}, found {count}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def apply(root: Path) -> None:
    internal_header = root / "avb.h"
    public_header = root / "include" / "esp_avb.h"
    source = root / "avb.c"
    atdecc = root / "atdecc.c"
    for path in (internal_header, public_header, source, atdecc):
        if not path.is_file():
            raise ValueError(f"missing exact-pin source: {path}")

    replace_once(internal_header, CONTROL_TAIL_OLD, CONTROL_TAIL_NEW, "control telemetry")
    replace_once(public_header, PUBLIC_LISTENER_TAIL_OLD, PUBLIC_LISTENER_TAIL_NEW, "public status")
    replace_once(source, STATUS_LISTENER_TAIL_OLD, STATUS_LISTENER_TAIL_NEW, "status copy")
    replace_once(atdecc, DEADLINE_FAULT_OLD, DEADLINE_FAULT_NEW, "deadline fault counter")
    replace_once(atdecc, UNMUTE_OLD, UNMUTE_NEW, "scheduler unmute timing")
    replace_once(atdecc, SCHEDULE_OLD, SCHEDULE_NEW, "scheduler epoch reset")
    replace_once(atdecc, MUTE_RESET_OLD, MUTE_RESET_NEW, "MUTE telemetry reset")


def check(root: Path) -> None:
    joined = "\n".join(
        (root / path).read_text(encoding="utf-8")
        for path in ("avb.h", "include/esp_avb.h", "avb.c", "atdecc.c")
    )
    required = (
        "uint64_t scheduler_unmute_ptp_ns;",
        "uint32_t scheduler_unmute_count;",
        "uint32_t deadline_fault_count;",
        "} aurora_endpoint_evidence;",
        "state->aurora_endpoint.scheduler_unmute_ptp_ns",
        "state->aurora_endpoint.scheduler_unmute_count",
        "state->aurora_endpoint.deadline_fault_count",
        "ctrl->scheduler_unmute_ptp_ns = now_ns;",
        "ctrl->scheduler_unmute_count++;",
        "state->aurora_endpoint.deadline_fault_count++;",
        "ctrl->scheduler_unmute_ptp_ns = 0;",
    )
    missing = [token for token in required if token not in joined]
    if missing:
        raise ValueError("endpoint telemetry patch missing: " + ", ".join(missing))


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    root.add_argument("command", choices=("apply", "check", "apply-check"))
    root.add_argument("--root", type=Path, required=True)
    return root


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command in ("apply", "apply-check"):
            apply(args.root)
        if args.command in ("check", "apply-check"):
            check(args.root)
    except (OSError, ValueError) as exc:
        print(f"aurora-esp-avb-endpoint-telemetry: FAIL: {exc}", file=sys.stderr)
        return 1
    print(
        "aurora-esp-avb-endpoint-telemetry: PASS "
        f"pin={PINNED_ESP_AVB_COMMIT} command={args.command} "
        "truth=scheduler-not-electrical"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
