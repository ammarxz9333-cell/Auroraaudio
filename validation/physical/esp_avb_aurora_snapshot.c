/*
 * Aurora exact-pin ESP-AVB physical-evidence snapshot helper.
 *
 * This file is intended to be compiled into the ESP32-P4 listener test
 * firmware after applying both Aurora validation patches to the pinned
 * esp_ptp and esp_avb components. It deliberately takes capture_unix_ms from
 * the host/controller request instead of assuming the ESP PTP timescale is
 * UTC. The resulting JSON is consumed by aurora_esp_avb_listener_evidence.py.
 */

#include "esp_avb.h"

#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define AURORA_ESP_SNAPSHOT_SCHEMA "aurora.esp-avb.listener-snapshot.v1"

static bool aurora_epoch_valid(const char *epoch) {
  if (!epoch)
    return false;
  size_t len = strlen(epoch);
  if (len == 0 || len > 64)
    return false;
  for (size_t i = 0; i < len; ++i) {
    char c = epoch[i];
    bool ok = (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
              (c >= '0' && c <= '9') || c == '.' || c == '_' || c == '-';
    if (!ok)
      return false;
  }
  return true;
}

static bool id_nonzero(const uint8_t id[8]) {
  uint8_t any = 0;
  for (size_t i = 0; i < 8; ++i)
    any |= id[i];
  return any != 0;
}

static void print_id(const uint8_t id[8]) {
  printf("%02x:%02x:%02x:%02x:%02x:%02x:%02x:%02x", id[0], id[1], id[2],
         id[3], id[4], id[5], id[6], id[7]);
}

/*
 * Print exactly one JSON object followed by a newline.
 *
 * capture_unix_ms MUST be supplied by the host/controller at request time.
 * This prevents an unverified PTP-vs-UTC timescale assumption from entering
 * the physical evidence. Use the exact epoch_id printed by Aurora's NXP host
 * probe. Return 0 only when the snapshot itself satisfies Aurora's strict
 * listener contract; a zero RX count before the send interval is allowed.
 */
int aurora_esp_avb_print_listener_snapshot(const char *epoch_id,
                                           uint64_t capture_unix_ms) {
  if (!aurora_epoch_valid(epoch_id) || capture_unix_ms == 0)
    return -1;

  avb_status_s status;
  memset(&status, 0, sizeof(status));
  int status_rc = avb_status(&status);

  bool stream_id_valid = id_nonzero(status.listener_evidence.stream_id);
  bool gm_valid = id_nonzero(status.grandmaster_id);
  bool media_ok = status.listener_evidence.sample_rate_hz == 48000 &&
                  status.listener_evidence.channels == 2 &&
                  status.listener_evidence.bit_depth == 24;
  bool gptp_locked = status.gptp_profile && status.clock_source_valid &&
                     status.gptp_clock_stable;
  bool pass = status_rc == 0 && gptp_locked && gm_valid &&
              status.listener_evidence.present &&
              status.listener_evidence.acmp_connected && stream_id_valid &&
              media_ok;

  printf("{\"schema\":\"%s\",\"verdict\":\"%s\",\"epoch_id\":\"%s\","
         "\"capture_unix_ms\":%" PRIu64 ",\"stream_id\":\"",
         AURORA_ESP_SNAPSHOT_SCHEMA, pass ? "PASS" : "FAIL", epoch_id,
         capture_unix_ms);
  print_id(status.listener_evidence.stream_id);
  printf("\",\"sample_rate_hz\":%" PRIu32
         ",\"channels\":%u,\"bit_depth\":%u,\"acmp_connected\":%s,"
         "\"gptp_locked\":%s,\"grandmaster_id\":\"",
         status.listener_evidence.sample_rate_hz,
         (unsigned)status.listener_evidence.channels,
         (unsigned)status.listener_evidence.bit_depth,
         status.listener_evidence.acmp_connected ? "true" : "false",
         gptp_locked ? "true" : "false");
  print_id(status.grandmaster_id);
  printf("\",\"rx_counter\":%" PRIu32 ",\"last_rx_us\":%" PRId64 "}\n",
         status.listener_evidence.frames_rx,
         status.listener_evidence.last_rx_us);

  return pass ? 0 : -1;
}
