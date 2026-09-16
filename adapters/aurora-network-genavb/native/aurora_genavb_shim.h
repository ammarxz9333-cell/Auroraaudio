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

uint32_t aurora_genavb_abi_version(void);
void *aurora_genavb_create(void);
int aurora_genavb_prepare(void *opaque, const struct aurora_genavb_config *config);
int aurora_genavb_start(void *opaque);
int aurora_genavb_submit(void *opaque, const uint8_t *aaf_payload, uint32_t payload_bytes,
                         uint64_t media_frame_index);
int aurora_genavb_stop(void *opaque);
int aurora_genavb_reset(void *opaque);
void aurora_genavb_destroy(void *opaque);

#ifdef __cplusplus
}
#endif

#endif
