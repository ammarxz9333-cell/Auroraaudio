#!/usr/bin/env python3
"""Add a PTP-deadline output gate to Aurora's exact-pinned ESP-AVB endpoint.

This patch is applied after aurora_patch_esp_avb_aem_endpoint.py. It turns the
AEM COMMIT state into a real absolute-PTP deadline scheduler at the firmware
source level. The default output-gate hook is weak/no-op so the exact pinned
endpoint continues to build without pretending that a particular amplifier or
codec mute pin has been wired. A board integration may override the hook.

Truth boundary: a green exact-pin firmware build proves source/API/link
compatibility of the scheduler and control state machine. It does not prove
physical PTP activation accuracy, RF timing, DAC/amplifier mute timing, or
acoustic synchronization.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

PINNED_ESP_AVB_COMMIT = "5e75bd3ed91b5407a254a5e49bfc18fc35e6cbb9"

SCHEDULER_CODE = r'''
#define AURORA_ENDPOINT_MAX_LATE_NS 2000000ULL

/* Board-specific endpoint integrations may override this weak hook with a
 * codec/Class-D mute control. The exact-pinned reference build intentionally
 * leaves it as a no-op: scheduler proof is not physical amplifier proof. */
__attribute__((weak)) void aurora_endpoint_set_output_muted(bool muted) {
  (void)muted;
}

static esp_timer_handle_t s_aurora_unmute_timer;
static avb_state_s *s_aurora_unmute_state;
static uint64_t s_aurora_unmute_epoch;

static uint64_t aurora_timespec_ns(const struct timespec *ts) {
  if (ts->tv_sec < 0 || ts->tv_nsec < 0)
    return 0;
  uint64_t sec = (uint64_t)ts->tv_sec;
  if (sec > (UINT64_MAX - (uint64_t)ts->tv_nsec) / 1000000000ULL)
    return UINT64_MAX;
  return sec * 1000000000ULL + (uint64_t)ts->tv_nsec;
}

static bool aurora_endpoint_ptp_ready(const avb_state_s *state) {
  return state->ptp_status.ptp_profile == ptp_profile_gptp &&
         state->ptp_status.clock_source_valid &&
         state->ptp_status.clock_stable;
}

static void aurora_endpoint_cancel_deadline(void) {
  if (s_aurora_unmute_timer != NULL)
    (void)esp_timer_stop(s_aurora_unmute_timer);
  s_aurora_unmute_state = NULL;
  s_aurora_unmute_epoch = 0;
  aurora_endpoint_set_output_muted(true);
}

static void aurora_endpoint_deadline_fault(avb_state_s *state,
                                           const char *reason) {
  aurora_endpoint_cancel_deadline();
  state->aurora_endpoint.phase = aurora_endpoint_fault;
  state->aurora_endpoint.muted = true;
  avbwarn("Aurora endpoint deadline fail-closed: %s", reason);
}

static void aurora_endpoint_unmute_deadline(void *arg) {
  avb_state_s *state = (avb_state_s *)arg;
  aurora_endpoint_control_s *ctrl = &state->aurora_endpoint;

  if (state != s_aurora_unmute_state ||
      ctrl->phase != aurora_endpoint_committed ||
      ctrl->active_epoch != s_aurora_unmute_epoch ||
      ctrl->armed_start_ptp_ns == 0 || !ctrl->muted) {
    return;
  }

  if (!aurora_endpoint_ptp_ready(state)) {
    aurora_endpoint_deadline_fault(state, "gPTP lost before activation");
    return;
  }

  struct timespec now = {0};
  ptpd_now(&now);
  uint64_t now_ns = aurora_timespec_ns(&now);
  if (now_ns == 0 || now_ns == UINT64_MAX) {
    aurora_endpoint_deadline_fault(state, "invalid PTP time at activation");
    return;
  }

  if (now_ns < ctrl->armed_start_ptp_ns) {
    uint64_t remaining_ns = ctrl->armed_start_ptp_ns - now_ns;
    uint64_t remaining_us = (remaining_ns + 999ULL) / 1000ULL;
    if (remaining_us == 0)
      remaining_us = 1;
    if (esp_timer_start_once(s_aurora_unmute_timer, remaining_us) != ESP_OK)
      aurora_endpoint_deadline_fault(state, "failed to re-arm deadline timer");
    return;
  }

  uint64_t late_ns = now_ns - ctrl->armed_start_ptp_ns;
  if (late_ns > AURORA_ENDPOINT_MAX_LATE_NS) {
    aurora_endpoint_deadline_fault(state, "activation deadline missed");
    return;
  }

  ctrl->muted = false;
  aurora_endpoint_set_output_muted(false);
  s_aurora_unmute_state = NULL;
  s_aurora_unmute_epoch = 0;
}

static bool aurora_endpoint_schedule_unmute(avb_state_s *state) {
  aurora_endpoint_control_s *ctrl = &state->aurora_endpoint;
  if (!aurora_endpoint_ptp_ready(state) || ctrl->armed_start_ptp_ns == 0)
    return false;

  struct timespec now = {0};
  ptpd_now(&now);
  uint64_t now_ns = aurora_timespec_ns(&now);
  if (now_ns == 0 || now_ns == UINT64_MAX)
    return false;

  if (s_aurora_unmute_timer == NULL) {
    const esp_timer_create_args_t args = {
        .callback = aurora_endpoint_unmute_deadline,
        .arg = state,
        .dispatch_method = ESP_TIMER_TASK,
        .name = "aurora_unmute",
        .skip_unhandled_events = true,
    };
    if (esp_timer_create(&args, &s_aurora_unmute_timer) != ESP_OK)
      return false;
  } else {
    (void)esp_timer_stop(s_aurora_unmute_timer);
  }

  s_aurora_unmute_state = state;
  s_aurora_unmute_epoch = ctrl->active_epoch;
  aurora_endpoint_set_output_muted(true);

  if (now_ns >= ctrl->armed_start_ptp_ns) {
    uint64_t late_ns = now_ns - ctrl->armed_start_ptp_ns;
    if (late_ns > AURORA_ENDPOINT_MAX_LATE_NS)
      return false;
    aurora_endpoint_unmute_deadline(state);
    return !ctrl->muted;
  }

  uint64_t remaining_ns = ctrl->armed_start_ptp_ns - now_ns;
  uint64_t remaining_us = (remaining_ns + 999ULL) / 1000ULL;
  if (remaining_us == 0)
    remaining_us = 1;
  return esp_timer_start_once(s_aurora_unmute_timer, remaining_us) == ESP_OK;
}

'''

FAIL_OLD = '''static uint8_t aurora_endpoint_fail(avb_state_s *state, const char *reason) {
  state->aurora_endpoint.phase = aurora_endpoint_fault;
  state->aurora_endpoint.muted = true;
  avbwarn("Aurora endpoint fail-closed: %s", reason);
  return aecp_status_bad_arguments;
}'''

FAIL_NEW = '''static uint8_t aurora_endpoint_fail(avb_state_s *state, const char *reason) {
  aurora_endpoint_cancel_deadline();
  state->aurora_endpoint.phase = aurora_endpoint_fault;
  state->aurora_endpoint.muted = true;
  avbwarn("Aurora endpoint fail-closed: %s", reason);
  return aecp_status_bad_arguments;
}'''

MUTE_OLD = '''    ctrl->phase = aurora_endpoint_muted;
    ctrl->muted = true;
    ctrl->last_command = command;
    return aecp_status_success;'''

MUTE_NEW = '''    ctrl->phase = aurora_endpoint_muted;
    ctrl->muted = true;
    aurora_endpoint_cancel_deadline();
    ctrl->last_command = command;
    return aecp_status_success;'''

COMMIT_OLD = '''    ctrl->phase = aurora_endpoint_committed;
    /* Remain muted. A separate PTP-time scheduler must perform the eventual
     * physical unmute; this exact-pin patch does not pretend it exists. */
    ctrl->muted = true;
    break;'''

COMMIT_NEW = '''    ctrl->phase = aurora_endpoint_committed;
    ctrl->muted = true;
    if (!aurora_endpoint_schedule_unmute(state))
      return aurora_endpoint_fail(state, "PTP deadline scheduler rejected COMMIT");
    break;'''

STOP_OLD = '''    ctrl->phase = aurora_endpoint_muted;
    ctrl->muted = true;
    ctrl->armed_start_ptp_ns = 0;
    break;'''

STOP_NEW = '''    ctrl->phase = aurora_endpoint_muted;
    ctrl->muted = true;
    ctrl->armed_start_ptp_ns = 0;
    aurora_endpoint_cancel_deadline();
    break;'''


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise ValueError(f"expected exactly one {label} anchor, found {count}")
    return text.replace(old, new, 1)


def apply(root: Path) -> None:
    atdecc = root / "atdecc.c"
    if not atdecc.is_file():
        raise ValueError("root does not look like an esp_avb checkout")

    text = atdecc.read_text(encoding="utf-8")
    if "aurora_endpoint_schedule_unmute" in text:
        raise ValueError("PTP deadline patch appears to be already applied")

    include_match = re.search(r"(?m)^#include ", text)
    if include_match is None:
        raise ValueError("could not find include block")
    includes = '#include "esp_timer.h"\n#include "esp_ptp.h"\n'
    text = text[: include_match.start()] + includes + text[include_match.start() :]

    marker = "static uint8_t aurora_endpoint_fail(avb_state_s *state, const char *reason) {"
    marker_count = text.count(marker)
    if marker_count != 1:
        raise ValueError(
            f"expected exactly one Aurora endpoint helper anchor, found {marker_count}"
        )
    text = text.replace(marker, SCHEDULER_CODE + marker, 1)

    text = replace_once(text, FAIL_OLD, FAIL_NEW, "fail-closed")
    text = replace_once(text, MUTE_OLD, MUTE_NEW, "MUTE")
    text = replace_once(text, COMMIT_OLD, COMMIT_NEW, "COMMIT")
    text = replace_once(text, STOP_OLD, STOP_NEW, "STOP")
    atdecc.write_text(text, encoding="utf-8")


def check(root: Path) -> None:
    text = (root / "atdecc.c").read_text(encoding="utf-8")
    required = (
        '#include "esp_timer.h"',
        '#include "esp_ptp.h"',
        "aurora_endpoint_set_output_muted",
        "aurora_endpoint_ptp_ready",
        "state->ptp_status.clock_stable",
        "ptpd_now(&now)",
        "esp_timer_start_once",
        "AURORA_ENDPOINT_MAX_LATE_NS",
        "gPTP lost before activation",
        'aurora_endpoint_fail(state, "PTP deadline scheduler rejected COMMIT")',
        "ctrl->muted = false;",
    )
    forbidden = (
        "A separate PTP-time scheduler must perform the eventual",
    )
    missing = [token for token in required if token not in text]
    stale = [token for token in forbidden if token in text]
    if missing:
        raise ValueError("PTP deadline patch missing: " + ", ".join(missing))
    if stale:
        raise ValueError("stale scheduler placeholder remains: " + ", ".join(stale))


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
        print(f"aurora-esp-avb-ptp-deadline: FAIL: {exc}", file=sys.stderr)
        return 1
    print(
        "aurora-esp-avb-ptp-deadline: PASS "
        f"pin={PINNED_ESP_AVB_COMMIT} command={args.command}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
