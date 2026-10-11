#!/usr/bin/env python3
"""Patch the exact pinned esp_avb with Aurora detachable AEM controls.

The patch preserves upstream controls 0-2 and adds Aurora controls 3-7 using
AECP/AEM SET_CONTROL and GET_CONTROL. It implements a fail-closed endpoint
control state machine only. It deliberately does not claim a physical
amplifier mute path or a PTP-time audio-unmute scheduler.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

PINNED_ESP_AVB_COMMIT = "5e75bd3ed91b5407a254a5e49bfc18fc35e6cbb9"

HEADER_COUNT_OLD = """#define AEM_NUM_CONTROLS 3          // IDENTIFY, Speaker Volume, Mic Gain"""
HEADER_COUNT_NEW = """#define AEM_NUM_CONTROLS 8          // upstream 0-2 + Aurora endpoint 3-7"""

HEADER_STATE_MARKER = """/* Main AVB state storage */
typedef struct avb_state_s {"""

HEADER_STATE_INSERT = """/* Aurora detachable endpoint control state.
 * This is volatile runtime state only. It is intentionally not persisted:
 * every boot starts fail-closed/muted and requires a fresh handover epoch. */
#define AURORA_ENDPOINT_ROLE_UNUSED 0xFF
#define AURORA_ENDPOINT_CMD_MUTE 1
#define AURORA_ENDPOINT_CMD_PREPARE 2
#define AURORA_ENDPOINT_CMD_ARM 3
#define AURORA_ENDPOINT_CMD_COMMIT 4
#define AURORA_ENDPOINT_CMD_STOP 5

typedef enum {
  aurora_endpoint_boot_muted = 0,
  aurora_endpoint_muted,
  aurora_endpoint_prepared,
  aurora_endpoint_armed,
  aurora_endpoint_committed,
  aurora_endpoint_fault,
} aurora_endpoint_phase_t;

typedef struct {
  uint64_t staged_epoch;
  uint8_t staged_role0;
  uint8_t staged_role1;
  uint64_t staged_start_ptp_ns;
  uint8_t last_command;

  uint64_t active_epoch;
  uint8_t active_role0;
  uint8_t active_role1;
  uint64_t armed_start_ptp_ns;
  aurora_endpoint_phase_t phase;
  bool muted;
} aurora_endpoint_control_s;

/* Main AVB state storage */
typedef struct avb_state_s {"""

HEADER_CONTROL_OLD = """  uint8_t ctrl_identify;                     // IDENTIFY control (0=off, 255=on)
  float ctrl_speaker_vol;                    // speaker volume in dB
  float ctrl_mic_gain;                       // mic gain in dB
  char descriptor_names[AVB_NAME_COUNT][64]; // user-settable descriptor names"""

HEADER_CONTROL_NEW = """  uint8_t ctrl_identify;                     // IDENTIFY control (0=off, 255=on)
  float ctrl_speaker_vol;                    // speaker volume in dB
  float ctrl_mic_gain;                       // mic gain in dB
  aurora_endpoint_control_s aurora_endpoint; // detachable endpoint staging/state
  char descriptor_names[AVB_NAME_COUNT][64]; // user-settable descriptor names"""

AVB_INIT_OLD = """  state->codec_enabled = false;

  if ((state->config.talker || state->config.listener) &&"""

AVB_INIT_NEW = """  state->codec_enabled = false;

  /* Aurora detachable endpoint control always boots fail-closed. This flag is
   * protocol state only; a physical amplifier-mute hook is a separate gate. */
  state->aurora_endpoint.phase = aurora_endpoint_boot_muted;
  state->aurora_endpoint.muted = true;
  state->aurora_endpoint.staged_role0 = AURORA_ENDPOINT_ROLE_UNUSED;
  state->aurora_endpoint.staged_role1 = AURORA_ENDPOINT_ROLE_UNUSED;
  state->aurora_endpoint.active_role0 = AURORA_ENDPOINT_ROLE_UNUSED;
  state->aurora_endpoint.active_role1 = AURORA_ENDPOINT_ROLE_UNUSED;

  if ((state->config.talker || state->config.listener) &&"""

ATDECC_FUNCTION_MARKER = """int avb_send_aecp_rsp_read_descr_control(avb_state_s *state,
                                         aecp_read_descriptor_rsp_s *msg,
                                         eth_addr_t *dest_addr) {"""

ATDECC_HELPERS = r"""static uint16_t aurora_control_value_len(uint16_t desc_index) {
  switch (desc_index) {
  case 0:
  case 4:
  case 5:
  case 7:
    return 1;
  case 1:
  case 2:
    return 2;
  case 3:
  case 6:
    return 8;
  default:
    return 0;
  }
}

static void aurora_control_type(uint8_t out[8], uint8_t tag) {
  /* Locally administered/private Aurora identifier. It is intentionally not
   * presented as an IEEE-assigned or vendor OUI. */
  const uint8_t base[8] = {0x02, 0x41, 0x55, 0x52, 0x4f, 0x52, 0x41, 0x00};
  memcpy(out, base, 8);
  out[7] = tag;
}

static void aurora_fill_control_u8(aem_control_desc_s *descriptor,
                                   const char *name, uint8_t tag,
                                   uint8_t current, uint8_t maximum) {
  uint16_t localized_description = 0xFFFF;
  uint16_t control_value_type = 1; /* CONTROL_LINEAR_UINT8 */
  uint8_t control_type[8];
  aurora_control_type(control_type, tag);
  strncpy((char *)descriptor->object_name, name,
          sizeof(descriptor->object_name) - 1);
  int_to_octets(&localized_description, descriptor->localized_description, 2);
  int_to_octets(&control_value_type, descriptor->control_value_type, 2);
  memcpy(descriptor->control_type, control_type, 8);

  /* LINEAR_UINT8: min, max, step, default, current, units(2), string_ref(2). */
  uint8_t *v = descriptor->value_details;
  v[0] = 0;
  v[1] = maximum;
  v[2] = 1;
  v[3] = 0;
  v[4] = current;
  v[5] = 0;
  v[6] = 0;
  v[7] = 0xFF;
  v[8] = 0xFF;
}

static void aurora_fill_control_u64(aem_control_desc_s *descriptor,
                                    const char *name, uint8_t tag,
                                    uint64_t current) {
  uint16_t localized_description = 0xFFFF;
  uint16_t control_value_type = 7; /* CONTROL_LINEAR_UINT64 */
  uint8_t control_type[8];
  uint64_t minimum = 0;
  uint64_t maximum = UINT64_MAX;
  uint64_t step = 1;
  uint64_t default_value = 0;
  aurora_control_type(control_type, tag);
  strncpy((char *)descriptor->object_name, name,
          sizeof(descriptor->object_name) - 1);
  int_to_octets(&localized_description, descriptor->localized_description, 2);
  int_to_octets(&control_value_type, descriptor->control_value_type, 2);
  memcpy(descriptor->control_type, control_type, 8);

  /* LINEAR_UINT64: five 64-bit scalars followed by units and string_ref. */
  uint8_t *v = descriptor->value_details;
  int_to_octets(&minimum, v + 0, 8);
  int_to_octets(&maximum, v + 8, 8);
  int_to_octets(&step, v + 16, 8);
  int_to_octets(&default_value, v + 24, 8);
  int_to_octets(&current, v + 32, 8);
  v[40] = 0;
  v[41] = 0;
  v[42] = 0xFF;
  v[43] = 0xFF;
}

static uint8_t aurora_endpoint_fail(avb_state_s *state, const char *reason) {
  state->aurora_endpoint.phase = aurora_endpoint_fault;
  state->aurora_endpoint.muted = true;
  avbwarn("Aurora endpoint fail-closed: %s", reason);
  return aecp_status_bad_arguments;
}

static bool aurora_endpoint_roles_valid(const aurora_endpoint_control_s *ctrl) {
  if (ctrl->staged_role0 > 11)
    return false;
  if (ctrl->staged_role1 != AURORA_ENDPOINT_ROLE_UNUSED &&
      ctrl->staged_role1 > 11)
    return false;
  if (ctrl->staged_role1 != AURORA_ENDPOINT_ROLE_UNUSED &&
      ctrl->staged_role1 == ctrl->staged_role0)
    return false;
  return true;
}

static uint8_t aurora_endpoint_apply_command(avb_state_s *state,
                                             uint8_t command) {
  aurora_endpoint_control_s *ctrl = &state->aurora_endpoint;

  if (command == AURORA_ENDPOINT_CMD_MUTE) {
    if (ctrl->staged_epoch == 0 || ctrl->staged_epoch < ctrl->active_epoch)
      return aurora_endpoint_fail(state, "stale/zero MUTE epoch");
    if (ctrl->phase == aurora_endpoint_fault &&
        ctrl->staged_epoch <= ctrl->active_epoch)
      return aurora_endpoint_fail(state, "fault recovery requires fresh epoch");

    ctrl->active_epoch = ctrl->staged_epoch;
    ctrl->active_role0 = AURORA_ENDPOINT_ROLE_UNUSED;
    ctrl->active_role1 = AURORA_ENDPOINT_ROLE_UNUSED;
    ctrl->armed_start_ptp_ns = 0;
    ctrl->phase = aurora_endpoint_muted;
    ctrl->muted = true;
    ctrl->last_command = command;
    return aecp_status_success;
  }

  if (ctrl->staged_epoch == 0 ||
      ctrl->staged_epoch != ctrl->active_epoch)
    return aurora_endpoint_fail(state, "command epoch mismatch");

  switch (command) {
  case AURORA_ENDPOINT_CMD_PREPARE:
    if (!aurora_endpoint_roles_valid(ctrl))
      return aurora_endpoint_fail(state, "invalid role assignment");
    if (ctrl->phase == aurora_endpoint_prepared &&
        ctrl->active_role0 == ctrl->staged_role0 &&
        ctrl->active_role1 == ctrl->staged_role1) {
      ctrl->last_command = command;
      return aecp_status_success; /* idempotent AECP retry */
    }
    if (ctrl->phase != aurora_endpoint_muted)
      return aurora_endpoint_fail(state, "PREPARE outside muted phase");
    ctrl->active_role0 = ctrl->staged_role0;
    ctrl->active_role1 = ctrl->staged_role1;
    ctrl->phase = aurora_endpoint_prepared;
    ctrl->muted = true;
    break;

  case AURORA_ENDPOINT_CMD_ARM:
    if (ctrl->staged_start_ptp_ns == 0)
      return aurora_endpoint_fail(state, "ARM without PTP activation time");
    if (ctrl->phase == aurora_endpoint_armed &&
        ctrl->armed_start_ptp_ns == ctrl->staged_start_ptp_ns) {
      ctrl->last_command = command;
      return aecp_status_success; /* idempotent AECP retry */
    }
    if (ctrl->phase != aurora_endpoint_prepared)
      return aurora_endpoint_fail(state, "ARM outside prepared phase");
    ctrl->armed_start_ptp_ns = ctrl->staged_start_ptp_ns;
    ctrl->phase = aurora_endpoint_armed;
    ctrl->muted = true;
    break;

  case AURORA_ENDPOINT_CMD_COMMIT:
    if (ctrl->phase == aurora_endpoint_committed) {
      ctrl->last_command = command;
      return aecp_status_success; /* idempotent AECP retry */
    }
    if (ctrl->phase != aurora_endpoint_armed)
      return aurora_endpoint_fail(state, "COMMIT outside armed phase");
    ctrl->phase = aurora_endpoint_committed;
    /* Remain muted. A separate PTP-time scheduler must perform the eventual
     * physical unmute; this exact-pin patch does not pretend it exists. */
    ctrl->muted = true;
    break;

  case AURORA_ENDPOINT_CMD_STOP:
    if (ctrl->phase == aurora_endpoint_fault)
      return aurora_endpoint_fail(state, "STOP cannot recover fault");
    ctrl->phase = aurora_endpoint_muted;
    ctrl->muted = true;
    ctrl->armed_start_ptp_ns = 0;
    break;

  default:
    return aurora_endpoint_fail(state, "unknown endpoint command");
  }

  ctrl->last_command = command;
  return aecp_status_success;
}

int avb_send_aecp_rsp_read_descr_control(avb_state_s *state,
                                         aecp_read_descriptor_rsp_s *msg,
                                         eth_addr_t *dest_addr) {"""

ATDECC_DESC_OLD = """    uint16_t value_string_ref = 20; /* strings desc 2, string_4: Gain */
    int_to_octets(&value_string_ref, v + 12, 2);
    break;
  }
  default:
    avbwarn("Unsupported control descriptor index: %d", index);
    return OK;
  }"""

ATDECC_DESC_NEW = """    uint16_t value_string_ref = 20; /* strings desc 2, string_4: Gain */
    int_to_octets(&value_string_ref, v + 12, 2);
    break;
  }
  case 3: /* Aurora handover epoch */
    aurora_fill_control_u64(&descriptor, "Aurora Epoch", 1,
                            state->aurora_endpoint.staged_epoch);
    break;
  case 4: /* Aurora rendered role lane 0 */
    aurora_fill_control_u8(&descriptor, "Aurora Role 0", 2,
                           state->aurora_endpoint.staged_role0, 0xFF);
    break;
  case 5: /* Aurora rendered role lane 1 */
    aurora_fill_control_u8(&descriptor, "Aurora Role 1", 3,
                           state->aurora_endpoint.staged_role1, 0xFF);
    break;
  case 6: /* Aurora absolute PTP activation time */
    aurora_fill_control_u64(&descriptor, "Aurora Start PTP ns", 4,
                            state->aurora_endpoint.staged_start_ptp_ns);
    break;
  case 7: /* Aurora endpoint command */
    aurora_fill_control_u8(&descriptor, "Aurora Command", 5,
                           state->aurora_endpoint.last_command,
                           AURORA_ENDPOINT_CMD_STOP);
    break;
  default:
    avbwarn("Unsupported control descriptor index: %d", index);
    return OK;
  }"""

SET_PRE_SWITCH_OLD = """  uint8_t *values = (uint8_t *)cmd + sizeof(aecp_aem_short_s);
  uint8_t status = aecp_status_success;

  switch (desc_index) {"""

SET_PRE_SWITCH_NEW = """  uint8_t *values = (uint8_t *)cmd + sizeof(aecp_aem_short_s);
  uint8_t status = aecp_status_success;
  uint16_t values_len = aurora_control_value_len(desc_index);
  uint16_t incoming_cdl =
      (msg->header.control_data_len_h << 8) | msg->header.control_data_len;
  uint16_t required_cdl = sizeof(aecp_common_s) + sizeof(aecp_common_aem_s) +
                          4 + values_len - AVTP_CDL_PREAMBLE_LEN;
  if (values_len == 0 || incoming_cdl < required_cdl)
    status = values_len == 0 ? aecp_status_no_such_descriptor
                             : aecp_status_bad_arguments;

  if (status == aecp_status_success)
  switch (desc_index) {"""

SET_CASES_OLD = """    avb_persist_request_save(state);
    break;
  }
  default:
    status = aecp_status_no_such_descriptor;
    break;
  }

  /* Build response: echo command with msg_type=response, set status */"""

SET_CASES_NEW = """    avb_persist_request_save(state);
    break;
  }
  case 3: /* Aurora epoch */
    state->aurora_endpoint.staged_epoch = octets_to_uint(values, 8);
    break;
  case 4: /* Aurora role lane 0 */
    state->aurora_endpoint.staged_role0 = values[0];
    break;
  case 5: /* Aurora role lane 1 */
    state->aurora_endpoint.staged_role1 = values[0];
    break;
  case 6: /* Aurora absolute PTP activation time */
    state->aurora_endpoint.staged_start_ptp_ns = octets_to_uint(values, 8);
    break;
  case 7: /* Aurora command: execute staged state transition last */
    status = aurora_endpoint_apply_command(state, values[0]);
    break;
  default:
    status = aecp_status_no_such_descriptor;
    break;
  }

  /* Build response: echo command with msg_type=response, set status */"""

SET_RESPONSE_OLD = """    case 2: {
      int16_t cur = (int16_t)(state->ctrl_mic_gain * 10.0f);
      values[0] = (cur >> 8) & 0xFF;
      values[1] = cur & 0xFF;
      break;
    }
    }
  }

  /* Compute exact response length from value size, not from the incoming
   * command length — some controllers send extra data (full value_details)
   * in the command that must not be echoed back. */
  uint16_t values_len = (desc_index == 0) ? 1 : 2;"""

SET_RESPONSE_NEW = """    case 2: {
      int16_t cur = (int16_t)(state->ctrl_mic_gain * 10.0f);
      values[0] = (cur >> 8) & 0xFF;
      values[1] = cur & 0xFF;
      break;
    }
    case 3:
      int_to_octets(&state->aurora_endpoint.staged_epoch, values, 8);
      break;
    case 4:
      values[0] = state->aurora_endpoint.staged_role0;
      break;
    case 5:
      values[0] = state->aurora_endpoint.staged_role1;
      break;
    case 6:
      int_to_octets(&state->aurora_endpoint.staged_start_ptp_ns, values, 8);
      break;
    case 7:
      values[0] = state->aurora_endpoint.last_command;
      break;
    }
  }

  /* Compute exact response length from value size, not from the incoming
   * command length — some controllers send extra data (full value_details)
   * in the command that must not be echoed back. */"""

GET_CASES_OLD = """  case 2: { /* Mic Gain */
    if (!state->config.talker) {
      status = aecp_status_not_supported;
      break;
    }
    int16_t cur = (int16_t)(state->ctrl_mic_gain * 10.0f);
    values[0] = (cur >> 8) & 0xFF;
    values[1] = cur & 0xFF;
    break;
  }
  default:
    status = aecp_status_no_such_descriptor;
    break;
  }"""

GET_CASES_NEW = """  case 2: { /* Mic Gain */
    if (!state->config.talker) {
      status = aecp_status_not_supported;
      break;
    }
    int16_t cur = (int16_t)(state->ctrl_mic_gain * 10.0f);
    values[0] = (cur >> 8) & 0xFF;
    values[1] = cur & 0xFF;
    break;
  }
  case 3:
    int_to_octets(&state->aurora_endpoint.staged_epoch, values, 8);
    break;
  case 4:
    values[0] = state->aurora_endpoint.staged_role0;
    break;
  case 5:
    values[0] = state->aurora_endpoint.staged_role1;
    break;
  case 6:
    int_to_octets(&state->aurora_endpoint.staged_start_ptp_ns, values, 8);
    break;
  case 7:
    values[0] = state->aurora_endpoint.last_command;
    break;
  default:
    status = aecp_status_no_such_descriptor;
    break;
  }"""

GET_LEN_OLD = """  /* Set control_data_len to include the values field */
  uint16_t values_len = (desc_index == 0) ? 1 : 2;
  uint16_t control_data_len = sizeof(aecp_common_s) +"""

GET_LEN_NEW = """  /* Set control_data_len to include the values field */
  uint16_t values_len = aurora_control_value_len(desc_index);
  if (values_len == 0)
    values_len = 1; /* bounded error response for unknown descriptors */
  uint16_t control_data_len = sizeof(aecp_common_s) +"""


def replace_exact(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise ValueError(f"expected exactly one source anchor in {path}, found {count}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def apply(root: Path) -> None:
    header = root / "avb.h"
    source = root / "avb.c"
    atdecc = root / "atdecc.c"
    if not header.is_file() or not source.is_file() or not atdecc.is_file():
        raise ValueError("root does not look like an esp_avb checkout")

    replace_exact(header, HEADER_COUNT_OLD, HEADER_COUNT_NEW)
    replace_exact(header, HEADER_STATE_MARKER, HEADER_STATE_INSERT)
    replace_exact(header, HEADER_CONTROL_OLD, HEADER_CONTROL_NEW)
    replace_exact(source, AVB_INIT_OLD, AVB_INIT_NEW)

    replace_exact(atdecc, ATDECC_FUNCTION_MARKER, ATDECC_HELPERS)
    replace_exact(atdecc, ATDECC_DESC_OLD, ATDECC_DESC_NEW)
    replace_exact(atdecc, SET_PRE_SWITCH_OLD, SET_PRE_SWITCH_NEW)
    replace_exact(atdecc, SET_CASES_OLD, SET_CASES_NEW)
    replace_exact(atdecc, SET_RESPONSE_OLD, SET_RESPONSE_NEW)
    replace_exact(atdecc, GET_CASES_OLD, GET_CASES_NEW)
    replace_exact(atdecc, GET_LEN_OLD, GET_LEN_NEW)


def check(root: Path) -> None:
    header = (root / "avb.h").read_text(encoding="utf-8")
    source = (root / "avb.c").read_text(encoding="utf-8")
    atdecc = (root / "atdecc.c").read_text(encoding="utf-8")
    required = (
        "#define AEM_NUM_CONTROLS 8",
        "aurora_endpoint_control_s aurora_endpoint;",
        "AURORA_ENDPOINT_CMD_COMMIT",
        "AURORA_ENDPOINT_ROLE_UNUSED",
        "CONTROL_LINEAR_UINT64",
        '"Aurora Epoch"',
        '"Aurora Start PTP ns"',
        "aurora_endpoint_apply_command",
        "fault recovery requires fresh epoch",
        "A separate PTP-time scheduler must perform the eventual",
        "state->aurora_endpoint.muted = true;",
        "state->aurora_endpoint.phase = aurora_endpoint_boot_muted;",
        "state->aurora_endpoint.staged_start_ptp_ns = octets_to_uint(values, 8)",
        "aurora_control_value_len(desc_index)",
    )
    joined = header + "\n" + source + "\n" + atdecc
    missing = [token for token in required if token not in joined]
    if missing:
        raise ValueError("Aurora AEM endpoint patch missing: " + ", ".join(missing))

    if "AEM_NUM_CONTROLS 3" in header:
        raise ValueError("old three-control descriptor count survived patch")


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
        print(f"aurora-esp-avb-aem-endpoint: FAIL: {exc}", file=sys.stderr)
        return 1
    print(
        "aurora-esp-avb-aem-endpoint: PASS "
        f"pin={PINNED_ESP_AVB_COMMIT} command={args.command} "
        "controls=3-7 scheduler=not-claimed"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
