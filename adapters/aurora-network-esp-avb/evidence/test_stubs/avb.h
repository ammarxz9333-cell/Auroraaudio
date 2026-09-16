#ifndef AURORA_TEST_AVB_H
#define AURORA_TEST_AVB_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <time.h>

#define AVB_MAX_NUM_INPUT_STREAMS 2
#define avtp_subtype_aaf 0x02

typedef struct {
    uint8_t subtype;
    uint8_t sample_rate;
    uint8_t format;
    uint8_t bit_depth;
    uint8_t chan_per_frame_h;
    uint8_t chan_per_frame;
} avtp_stream_format_aaf_pcm_s;

typedef union {
    uint8_t subtype;
    avtp_stream_format_aaf_pcm_s aaf_pcm;
} avtp_stream_format_s;

typedef struct {
    uint8_t stream_id[8];
    bool connected;
    avtp_stream_format_s stream_format;
} avb_listener_stream_s;

typedef struct {
    bool listener;
} avb_config_s;

typedef struct avb_state_s {
    avb_config_s config;
    avb_listener_stream_s input_streams[AVB_MAX_NUM_INPUT_STREAMS];
    size_t num_input_streams;
} avb_state_s;

typedef struct {
    uint8_t frames_rx : 1;
} aem_stream_in_counters_val_s;

typedef struct {
    uint8_t frames_rx[4];
} aem_stream_in_counters_s;

extern avb_state_s *s_state;

void avb_get_stream_in_counters(aem_stream_in_counters_val_s *valid,
                                aem_stream_in_counters_s *counters);
uint32_t aaf_code_to_sample_rate(uint8_t code);
int64_t avb_stream_in_last_rx_us(avb_state_s *state, uint16_t index);
uint64_t octets_to_uint(const uint8_t *buffer, size_t size);
int64_t timespec_to_ms(const struct timespec *ts);

#endif
