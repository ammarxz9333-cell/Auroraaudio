#!/usr/bin/env python3
"""Verify the pinned esp_avb/esp_ptp source contract used by Aurora evidence.

This is source/API compatibility evidence only. It never substitutes for an
ESP-IDF build or physical endpoint run.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path


def require(path: Path, needles: list[str], failures: list[str]) -> None:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        failures.append(f"cannot read {path}: {exc}")
        return
    for needle in needles:
        if needle not in text:
            failures.append(f"{path}: missing exact-pin contract fragment {needle!r}")


def verify(avb: Path, ptp: Path) -> list[str]:
    failures: list[str] = []

    require(
        avb / "CMakeLists.txt",
        [
            'INCLUDE_DIRS "." "./include"',
            "avtp.c",
            "atdecc.c",
        ],
        failures,
    )
    require(
        avb / "include" / "esp_avb.h",
        [
            "bool clock_source_valid;",
            "bool streaming_in;",
            "uint32_t sample_rate;",
            "int avb_status(avb_status_s *status);",
        ],
        failures,
    )
    require(
        avb / "avb.h",
        [
            "unique_id_t stream_id;",
            "bool connected;",
            "avb_listener_stream_s input_streams[AVB_MAX_NUM_INPUT_STREAMS];",
            "void avb_get_stream_in_counters(aem_stream_in_counters_val_s *valid,",
            "int64_t avb_stream_in_last_rx_us(avb_state_s *state, uint16_t index);",
            "uint32_t aaf_code_to_sample_rate(uint8_t code);",
        ],
        failures,
    )
    require(
        avb / "avb.c",
        [
            "status->clock_source_valid = state->ptp_status.clock_source_valid;",
            "if (state->input_streams[i].connected)",
            "status->streaming_in = true;",
            "ptpd_status(0, &ptp_status)",
        ],
        failures,
    )
    require(
        avb / "avtp.c",
        [
            "void avb_get_stream_in_counters(aem_stream_in_counters_val_s *valid,",
            "counter_val = ctx->pkt_count;",
            "int_to_octets(&counter_val, counters->frames_rx, 4);",
            "int64_t avb_stream_in_last_rx_us(avb_state_s *state, uint16_t index)",
        ],
        failures,
    )
    require(
        avb / "atdecc.c",
        [
            "avb_send_aecp_rsp_get_stream_info",
            "not implemented",
            "avb_send_aecp_rsp_get_counters",
        ],
        failures,
    )
    require(
        ptp / "include" / "esp_ptp.h",
        [
            "bool clock_source_valid;",
            "clock_info_s clock_source_info;",
            "struct timespec last_received_sync;",
            "int ptpd_status(int pid, FAR struct ptpd_status_s *status);",
            "ptp_profile_gptp = 1",
        ],
        failures,
    )
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--esp-avb", type=Path, required=True)
    parser.add_argument("--esp-ptp", type=Path, required=True)
    args = parser.parse_args()

    failures = verify(args.esp_avb, args.esp_ptp)
    if failures:
        for failure in failures:
            print(f"FAIL: {failure}", file=sys.stderr)
        return 1

    print(
        "aurora-esp-avb-evidence-pin: PASS "
        "status=public ptp=public stream=pin-coupled frames_rx=actual-packets get-counters-wire=not-claimed"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
