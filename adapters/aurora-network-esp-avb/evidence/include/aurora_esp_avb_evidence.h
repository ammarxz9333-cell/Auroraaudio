#ifndef AURORA_ESP_AVB_EVIDENCE_H
#define AURORA_ESP_AVB_EVIDENCE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define AURORA_ESP_AVB_SNAPSHOT_SCHEMA "aurora.genavb.esp-listener-snapshot.v1"
#define AURORA_ESP_AVB_ID_BYTES 8u

typedef struct {
    uint64_t sample_unix_ms;
    uint8_t entity_id[AURORA_ESP_AVB_ID_BYTES];
    uint8_t stream_id[AURORA_ESP_AVB_ID_BYTES];
    uint8_t grandmaster_id[AURORA_ESP_AVB_ID_BYTES];
    bool acmp_connected;
    bool gptp_locked;
    uint32_t sample_rate_hz;
    uint16_t channels;
    uint8_t bit_depth;
    uint64_t rx_frames;
    int64_t last_rx_us;
    int64_t last_sync_monotonic_ms;
} aurora_esp_avb_listener_snapshot_t;

/* Collect one control-thread snapshot from the exact-pinned esp_avb/esp_ptp
 * runtime. This function is not realtime-callback safe and must not be called
 * from an audio/AVTP hot path. It returns 0 when the state was collected; the
 * booleans/format/counters in the snapshot determine whether the physical
 * evidence itself is acceptable. */
int aurora_esp_avb_listener_snapshot(aurora_esp_avb_listener_snapshot_t *snapshot);

/* Serialize a collected snapshot as one JSON object. Returns the number of
 * bytes excluding the terminating NUL on success, or a negative value when the
 * buffer is invalid/too small. */
int aurora_esp_avb_listener_snapshot_json(
    const aurora_esp_avb_listener_snapshot_t *snapshot,
    char *buffer,
    size_t capacity);

#ifdef __cplusplus
}
#endif

#endif /* AURORA_ESP_AVB_EVIDENCE_H */
