#define _POSIX_C_SOURCE 200809L

#include "aurora_esp_avb_evidence.h"

#include <inttypes.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

#include "avb.h"
#include "esp_avb.h"
#include "esp_ptp.h"

/* esp_avb intentionally exposes its component root as an include directory,
 * but s_state is an implementation global rather than part of esp_avb.h.
 * Aurora pins the exact upstream commit before using this narrow evidence
 * adapter, so this one declaration is the deliberate pin-coupled boundary. */
extern avb_state_s *s_state;

#define AUDIO_INPUT_INDEX 0u
#define EXPECTED_RATE_HZ 48000u
#define EXPECTED_CHANNELS 2u
#define EXPECTED_BIT_DEPTH 24u

static bool id_nonzero(const uint8_t id[AURORA_ESP_AVB_ID_BYTES])
{
    size_t index;

    for (index = 0; index < AURORA_ESP_AVB_ID_BYTES; index++) {
        if (id[index] != 0)
            return true;
    }
    return false;
}

static uint64_t realtime_ms(void)
{
    struct timespec ts;

    if (clock_gettime(CLOCK_REALTIME, &ts) != 0)
        return 0;

    return (uint64_t)ts.tv_sec * 1000ULL + (uint64_t)ts.tv_nsec / 1000000ULL;
}

static uint16_t aaf_channels(const avtp_stream_format_aaf_pcm_s *format)
{
    return (uint16_t)(((uint16_t)format->chan_per_frame_h << 2) |
                      (uint16_t)format->chan_per_frame);
}

static void format_id(const uint8_t id[AURORA_ESP_AVB_ID_BYTES],
                      char output[3 * AURORA_ESP_AVB_ID_BYTES])
{
    size_t index;
    char *cursor = output;

    for (index = 0; index < AURORA_ESP_AVB_ID_BYTES; index++) {
        int written = snprintf(cursor, 4, "%02x%s", id[index],
                               index + 1 < AURORA_ESP_AVB_ID_BYTES ? ":" : "");
        if (written < 0)
            return;
        cursor += written;
    }
}

int aurora_esp_avb_listener_snapshot(aurora_esp_avb_listener_snapshot_t *snapshot)
{
    avb_status_s status;
    struct ptpd_status_s ptp_status;
    aem_stream_in_counters_val_s counters_valid;
    aem_stream_in_counters_s counters;
    avb_state_s *state;
    avb_listener_stream_s *stream;
    const avtp_stream_format_aaf_pcm_s *aaf;
    uint64_t sampled_at;

    if (!snapshot)
        return -1;

    memset(snapshot, 0, sizeof(*snapshot));
    memset(&status, 0, sizeof(status));
    memset(&ptp_status, 0, sizeof(ptp_status));
    memset(&counters_valid, 0, sizeof(counters_valid));
    memset(&counters, 0, sizeof(counters));

    sampled_at = realtime_ms();
    if (!sampled_at)
        return -2;
    if (avb_status(&status) != 0)
        return -3;
    if (ptpd_status(0, &ptp_status) != 0)
        return -4;

    state = s_state;
    if (!state || !state->config.listener || state->num_input_streams == 0)
        return -5;

    stream = &state->input_streams[AUDIO_INPUT_INDEX];
    if (stream->stream_format.subtype != avtp_subtype_aaf)
        return -6;

    aaf = &stream->stream_format.aaf_pcm;
    avb_get_stream_in_counters(&counters_valid, &counters);

    snapshot->sample_unix_ms = sampled_at;
    memcpy(snapshot->entity_id, status.entity.id, sizeof(snapshot->entity_id));
    memcpy(snapshot->stream_id, stream->stream_id, sizeof(snapshot->stream_id));
    memcpy(snapshot->grandmaster_id, ptp_status.clock_source_info.id,
           sizeof(snapshot->grandmaster_id));
    snapshot->acmp_connected = stream->connected;
    snapshot->sample_rate_hz = aaf_code_to_sample_rate(aaf->sample_rate);
    snapshot->channels = aaf_channels(aaf);
    snapshot->bit_depth = aaf->bit_depth;
    snapshot->rx_frames = counters_valid.frames_rx
                              ? octets_to_uint(counters.frames_rx,
                                               sizeof(counters.frames_rx))
                              : 0;
    snapshot->last_rx_us = avb_stream_in_last_rx_us(state, AUDIO_INPUT_INDEX);
    snapshot->last_sync_monotonic_ms = timespec_to_ms(&ptp_status.last_received_sync);
    snapshot->gptp_locked = status.clock_source_valid &&
                            ptp_status.clock_source_valid &&
                            ptp_status.ptp_profile == ptp_profile_gptp &&
                            id_nonzero(snapshot->grandmaster_id) &&
                            snapshot->last_sync_monotonic_ms > 0;

    return 0;
}

int aurora_esp_avb_listener_snapshot_json(
    const aurora_esp_avb_listener_snapshot_t *snapshot,
    char *buffer,
    size_t capacity)
{
    char entity_id[3 * AURORA_ESP_AVB_ID_BYTES] = {0};
    char stream_id[3 * AURORA_ESP_AVB_ID_BYTES] = {0};
    char gm_id[3 * AURORA_ESP_AVB_ID_BYTES] = {0};
    bool ready;
    int written;

    if (!snapshot || !buffer || capacity == 0)
        return -1;

    format_id(snapshot->entity_id, entity_id);
    format_id(snapshot->stream_id, stream_id);
    format_id(snapshot->grandmaster_id, gm_id);

    ready = snapshot->acmp_connected && snapshot->gptp_locked &&
            id_nonzero(snapshot->stream_id) &&
            snapshot->sample_rate_hz == EXPECTED_RATE_HZ &&
            snapshot->channels == EXPECTED_CHANNELS &&
            snapshot->bit_depth == EXPECTED_BIT_DEPTH;

    written = snprintf(
        buffer, capacity,
        "{\"schema\":\"%s\",\"verdict\":\"%s\",\"sample_unix_ms\":%" PRIu64
        ",\"entity_id\":\"%s\",\"stream_id\":\"%s\",\"grandmaster_id\":\"%s\""
        ",\"acmp_connected\":%s,\"gptp_locked\":%s,\"sample_rate_hz\":%" PRIu32
        ",\"channels\":%u,\"bit_depth\":%u,\"rx_frames\":%" PRIu64
        ",\"last_rx_us\":%" PRId64 ",\"last_sync_monotonic_ms\":%" PRId64 "}",
        AURORA_ESP_AVB_SNAPSHOT_SCHEMA, ready ? "PASS" : "FAIL",
        snapshot->sample_unix_ms, entity_id, stream_id, gm_id,
        snapshot->acmp_connected ? "true" : "false",
        snapshot->gptp_locked ? "true" : "false",
        snapshot->sample_rate_hz, (unsigned int)snapshot->channels,
        (unsigned int)snapshot->bit_depth, snapshot->rx_frames,
        snapshot->last_rx_us, snapshot->last_sync_monotonic_ms);

    if (written < 0 || (size_t)written >= capacity)
        return -2;
    return written;
}
