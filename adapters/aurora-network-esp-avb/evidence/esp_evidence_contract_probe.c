#include "aurora_esp_avb_evidence.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "avb.h"
#include "esp_avb.h"
#include "esp_ptp.h"

static avb_state_s mock_state;
avb_state_s *s_state = &mock_state;
static uint32_t mock_frames_rx = 500;
static int64_t mock_last_rx_us = 1234567;
static bool mock_avb_clock_valid = true;
static bool mock_ptp_clock_valid = true;
static uint8_t mock_gm_id[8] = {0x02, 0x00, 0x00, 0xff, 0xfe, 0x00, 0x00, 0x01};
static ptp_profile_e mock_profile = ptp_profile_gptp;
static struct timespec mock_last_sync = {.tv_sec = 10, .tv_nsec = 0};

int avb_status(avb_status_s *status)
{
    static const uint8_t entity_id[8] = {0x02, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x01};

    memset(status, 0, sizeof(*status));
    status->clock_source_valid = mock_avb_clock_valid;
    status->streaming_in = mock_state.input_streams[0].connected;
    status->sample_rate = 48000;
    memcpy(status->entity.id, entity_id, sizeof(entity_id));
    return 0;
}

int ptpd_status(int pid, struct ptpd_status_s *status)
{
    (void)pid;
    memset(status, 0, sizeof(*status));
    status->ptp_profile = mock_profile;
    status->clock_source_valid = mock_ptp_clock_valid;
    memcpy(status->clock_source_info.id, mock_gm_id, sizeof(mock_gm_id));
    status->last_received_sync = mock_last_sync;
    return 0;
}

void avb_get_stream_in_counters(aem_stream_in_counters_val_s *valid,
                                aem_stream_in_counters_s *counters)
{
    memset(valid, 0, sizeof(*valid));
    memset(counters, 0, sizeof(*counters));
    valid->frames_rx = true;
    counters->frames_rx[0] = (uint8_t)(mock_frames_rx >> 24);
    counters->frames_rx[1] = (uint8_t)(mock_frames_rx >> 16);
    counters->frames_rx[2] = (uint8_t)(mock_frames_rx >> 8);
    counters->frames_rx[3] = (uint8_t)mock_frames_rx;
}

uint32_t aaf_code_to_sample_rate(uint8_t code)
{
    return code == 5 ? 48000u : 0u;
}

int64_t avb_stream_in_last_rx_us(avb_state_s *state, uint16_t index)
{
    (void)state;
    (void)index;
    return mock_last_rx_us;
}

uint64_t octets_to_uint(const uint8_t *buffer, size_t size)
{
    uint64_t value = 0;
    size_t index;

    for (index = 0; index < size; index++)
        value = (value << 8) | buffer[index];
    return value;
}

int64_t timespec_to_ms(const struct timespec *ts)
{
    return (int64_t)ts->tv_sec * 1000 + ts->tv_nsec / 1000000;
}

static void configure(const char *scenario)
{
    static const uint8_t stream_id[8] = {0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x0a};
    avb_listener_stream_s *stream;

    memset(&mock_state, 0, sizeof(mock_state));
    mock_state.config.listener = true;
    mock_state.num_input_streams = 1;
    stream = &mock_state.input_streams[0];
    stream->connected = true;
    memcpy(stream->stream_id, stream_id, sizeof(stream_id));
    stream->stream_format.aaf_pcm.subtype = avtp_subtype_aaf;
    stream->stream_format.aaf_pcm.sample_rate = 5;
    stream->stream_format.aaf_pcm.bit_depth = 24;
    stream->stream_format.aaf_pcm.chan_per_frame_h = 0;
    stream->stream_format.aaf_pcm.chan_per_frame = 2;

    mock_frames_rx = 500;
    mock_last_rx_us = 1234567;
    mock_avb_clock_valid = true;
    mock_ptp_clock_valid = true;
    mock_profile = ptp_profile_gptp;
    mock_last_sync.tv_sec = 10;
    mock_last_sync.tv_nsec = 0;
    mock_gm_id[0] = 0x02;
    mock_gm_id[1] = 0x00;
    mock_gm_id[2] = 0x00;
    mock_gm_id[3] = 0xff;
    mock_gm_id[4] = 0xfe;
    mock_gm_id[5] = 0x00;
    mock_gm_id[6] = 0x00;
    mock_gm_id[7] = 0x01;

    if (strcmp(scenario, "disconnected") == 0) {
        stream->connected = false;
    } else if (strcmp(scenario, "gptp-invalid") == 0) {
        mock_ptp_clock_valid = false;
    } else if (strcmp(scenario, "profile-wrong") == 0) {
        mock_profile = ptp_profile_standard;
    } else if (strcmp(scenario, "gm-zero") == 0) {
        memset(mock_gm_id, 0, sizeof(mock_gm_id));
    } else if (strcmp(scenario, "format-wrong") == 0) {
        stream->stream_format.aaf_pcm.bit_depth = 32;
    } else if (strcmp(scenario, "stream-zero") == 0) {
        memset(stream->stream_id, 0, sizeof(stream->stream_id));
    } else if (strcmp(scenario, "no-listener") == 0) {
        mock_state.config.listener = false;
    }
}

int main(int argc, char **argv)
{
    aurora_esp_avb_listener_snapshot_t snapshot;
    char json[1024];
    const char *scenario = argc > 1 ? argv[1] : "healthy";
    int rc;

    configure(scenario);
    rc = aurora_esp_avb_listener_snapshot(&snapshot);
    if (rc != 0) {
        printf("aurora-esp-avb-evidence: COLLECT_FAIL scenario=%s rc=%d\n", scenario, rc);
        return 1;
    }

    rc = aurora_esp_avb_listener_snapshot_json(&snapshot, json, sizeof(json));
    if (rc < 0) {
        fprintf(stderr, "JSON serialization failed: %d\n", rc);
        return 1;
    }
    puts(json);

    return strstr(json, "\"verdict\":\"PASS\"") != NULL ? 0 : 1;
}
