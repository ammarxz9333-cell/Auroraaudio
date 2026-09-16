#!/usr/bin/env python3
"""Apply Aurora's narrow listener-evidence instrumentation to pinned esp_avb.

This is intentionally an exact-source transformation, not a floating fork. It
adds evidence fields to the existing thread-safe avb_status() path so physical
validation can read ACMP stream identity, actual STREAM_INPUT frame count,
last-RX time, the selected gPTP grandmaster/BTC identity, and the separately
instrumented esp_ptp servo-stability result without claiming the pinned
GET_COUNTERS ATDECC stubs are implemented.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

PINNED_ESP_AVB_COMMIT = "5e75bd3ed91b5407a254a5e49bfc18fc35e6cbb9"

HEADER_OLD = """  bool streaming_out;      // one or more output streams are active\n  uint32_t sample_rate;    // current media sample rate in Hz\n  struct {\n    uint8_t id[8]; // Entity ID\n  } entity;\n"""

HEADER_NEW = """  bool streaming_out;      // one or more output streams are active\n  uint32_t sample_rate;    // current media sample rate in Hz\n\n  /* Aurora physical-validation evidence. Populated by the same serialized\n   * avb_status() request path as the fields above; no direct cross-core state\n   * walk is required by the application. Input stream 0 is the audio listener\n   * at this exact pin (input stream 1 is CRF when listener audio is enabled). */\n  bool gptp_profile;\n  bool gptp_clock_stable;\n  uint8_t grandmaster_id[8];\n  struct {\n    bool present;\n    bool acmp_connected;\n    uint8_t stream_id[8];\n    uint32_t frames_rx;\n    int64_t last_rx_us;\n    uint32_t sample_rate_hz;\n    uint8_t channels;\n    uint8_t bit_depth;\n  } listener_evidence;\n\n  struct {\n    uint8_t id[8]; // Entity ID\n  } entity;\n"""

SOURCE_OLD = """  status->clock_source_valid = state->ptp_status.clock_source_valid;\n  status->avb_lite = state->avb_lite;\n  /* Runtime rate changes write back to config.default_sample_rate,\n   * so it is the current media rate, same source the AUDIO_UNIT\n   * descriptor reports as current_sampling_rate. */\n  status->sample_rate = state->config.default_sample_rate;\n"""

SOURCE_NEW = """  status->clock_source_valid = state->ptp_status.clock_source_valid;\n  status->avb_lite = state->avb_lite;\n  /* Runtime rate changes write back to config.default_sample_rate,\n   * so it is the current media rate, same source the AUDIO_UNIT\n   * descriptor reports as current_sampling_rate. */\n  status->sample_rate = state->config.default_sample_rate;\n\n  status->gptp_profile = state->ptp_status.ptp_profile == ptp_profile_gptp;\n  status->gptp_clock_stable = state->ptp_status.clock_stable;\n  /* clock_source_info.id is the selected announce sender. The gPTP\n   * grandmaster/BTC identity propagated across bridges is btc_id. */\n  memcpy(status->grandmaster_id, state->ptp_status.clock_source_info.btc_id,\n         sizeof(status->grandmaster_id));\n  memset(&status->listener_evidence, 0, sizeof(status->listener_evidence));\n\n  /* At this pin STREAM_INPUT[0] is the audio listener whenever listener\n   * support is enabled. Read its identity from the ACMP-owned stream state,\n   * actual frame count from the existing internal Milan counter producer, and\n   * the last real stream-frame arrival from avtp.c. This does not pretend the\n   * ATDECC GET_COUNTERS command/response stubs are implemented. */\n  if (state->config.listener && state->num_input_streams > 0) {\n    avb_listener_stream_s *stream = &state->input_streams[0];\n    status->listener_evidence.present = true;\n    status->listener_evidence.acmp_connected = stream->connected;\n    memcpy(status->listener_evidence.stream_id, stream->stream_id,\n           sizeof(status->listener_evidence.stream_id));\n    status->listener_evidence.last_rx_us = avb_stream_in_last_rx_us(state, 0);\n\n    if (stream->stream_format.subtype == avtp_subtype_aaf) {\n      status->listener_evidence.sample_rate_hz =\n          aaf_code_to_sample_rate(stream->stream_format.aaf_pcm.sample_rate);\n      status->listener_evidence.channels =\n          (uint8_t)(((uint16_t)stream->stream_format.aaf_pcm.chan_per_frame_h << 2) |\n                    stream->stream_format.aaf_pcm.chan_per_frame);\n      status->listener_evidence.bit_depth =\n          stream->stream_format.aaf_pcm.bit_depth;\n    }\n\n    aem_stream_in_counters_val_s valid;\n    aem_stream_in_counters_s counters;\n    avb_get_stream_in_counters(&valid, &counters);\n    if (valid.frames_rx) {\n      status->listener_evidence.frames_rx =\n          (uint32_t)octets_to_uint(counters.frames_rx, 4);\n    }\n  }\n"""


def replace_exact(path: Path, old: str, new: str) -> None:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise ValueError(f"cannot read {path}: {exc}") from exc
    count = text.count(old)
    if count != 1:
        raise ValueError(f"expected exactly one source anchor in {path}, found {count}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def apply(root: Path) -> None:
    header = root / "include" / "esp_avb.h"
    source = root / "avb.c"
    if not header.is_file() or not source.is_file():
        raise ValueError("root does not look like an esp_avb checkout")
    replace_exact(header, HEADER_OLD, HEADER_NEW)
    replace_exact(source, SOURCE_OLD, SOURCE_NEW)


def check(root: Path) -> None:
    header = (root / "include" / "esp_avb.h").read_text(encoding="utf-8")
    source = (root / "avb.c").read_text(encoding="utf-8")
    required_header = (
        "bool gptp_profile;",
        "bool gptp_clock_stable;",
        "uint8_t grandmaster_id[8];",
        "uint32_t frames_rx;",
        "int64_t last_rx_us;",
        "uint8_t stream_id[8];",
    )
    required_source = (
        "state->ptp_status.clock_stable",
        "state->ptp_status.clock_source_info.btc_id",
        "stream->connected",
        "avb_stream_in_last_rx_us(state, 0)",
        "avb_get_stream_in_counters(&valid, &counters)",
        "octets_to_uint(counters.frames_rx, 4)",
    )
    missing = [token for token in required_header if token not in header]
    missing += [token for token in required_source if token not in source]
    if missing:
        raise ValueError("instrumentation check missing: " + ", ".join(missing))


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
        print(f"aurora-esp-avb-listener-instrumentation: FAIL: {exc}", file=sys.stderr)
        return 1
    print(
        "aurora-esp-avb-listener-instrumentation: PASS "
        f"pin={PINNED_ESP_AVB_COMMIT} command={args.command}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
