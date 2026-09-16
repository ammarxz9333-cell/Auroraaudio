#ifndef AURORA_TEST_ESP_AVB_H
#define AURORA_TEST_ESP_AVB_H

#include <stdbool.h>
#include <stdint.h>

typedef struct {
    bool clock_source_valid;
    bool avb_lite;
    bool streaming_in;
    bool streaming_out;
    uint32_t sample_rate;
    struct {
        uint8_t id[8];
    } entity;
} avb_status_s;

int avb_status(avb_status_s *status);

#endif
