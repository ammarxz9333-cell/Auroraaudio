#ifndef AURORA_GENAVB_SHIM_H
#define AURORA_GENAVB_SHIM_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define AURORA_GENAVB_SHIM_ABI_VERSION 1u

struct aurora_genavb_config {
    uint16_t port;
    uint16_t reserved;
    uint8_t stream_id[8];
    uint8_t destination_mac[6];
    uint8_t padding[2];
    uint32_t target_latency_frames;
    uint32_t block_frames;
};

enum aurora_genavb_avdecc_event_kind {
    AURORA_GENAVB_AVDECC_EVENT_NONE = 0,
    AURORA_GENAVB_AVDECC_EVENT_CONNECT = 1,
    AURORA_GENAVB_AVDECC_EVENT_DISCONNECT = 2,
};

struct aurora_genavb_avdecc_event {
    uint32_t kind;
    uint16_t stream_index;
    uint16_t port;
    uint16_t direction;
    uint16_t stream_class;
    uint8_t stream_id[8];
    uint8_t destination_mac[6];
    uint8_t reserved[2];
    uint32_t sample_rate_hz;
    uint32_t channels;
    uint32_t bit_depth;
};

uint32_t aurora_genavb_abi_version(void);
void *aurora_genavb_create(void);
int aurora_genavb_prepare(void *opaque, const struct aurora_genavb_config *config);
int aurora_genavb_start(void *opaque);
int aurora_genavb_submit(void *opaque, const uint8_t *aaf_payload, uint32_t payload_bytes,
                         uint64_t media_frame_index);
int aurora_genavb_stop(void *opaque);
int aurora_genavb_reset(void *opaque);
void aurora_genavb_destroy(void *opaque);

/* Worker/control-thread AVDECC media-stack adapter. These calls are never
 * valid from Aurora's realtime callback. The GenAVB stack remains the ACMP /
 * AVDECC connection-state owner; Aurora only consumes sanitized connect and
 * disconnect indications. */
void *aurora_genavb_avdecc_create(void);
int aurora_genavb_avdecc_open(void *opaque);
int aurora_genavb_avdecc_rx_fd(void *opaque);
int aurora_genavb_avdecc_receive(void *opaque, struct aurora_genavb_avdecc_event *event);
int aurora_genavb_avdecc_close(void *opaque);
void aurora_genavb_avdecc_destroy(void *opaque);

#ifdef __cplusplus
}
#endif

#endif
