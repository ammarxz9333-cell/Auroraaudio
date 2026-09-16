#ifndef AURORA_TEST_ESP_PTP_H
#define AURORA_TEST_ESP_PTP_H

#include <stdbool.h>
#include <stdint.h>
#include <time.h>

typedef enum {
    ptp_profile_standard = 0,
    ptp_profile_gptp = 1,
} ptp_profile_e;

typedef struct {
    uint8_t id[8];
} clock_info_s;

struct ptpd_status_s {
    ptp_profile_e ptp_profile;
    bool clock_source_valid;
    clock_info_s clock_source_info;
    struct timespec last_received_sync;
};

int ptpd_status(int pid, struct ptpd_status_s *status);

#endif
