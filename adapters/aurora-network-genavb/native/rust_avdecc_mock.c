#include <stdint.h>
#include <stdlib.h>

#define AURORA_GENAVB_SHIM_ABI_VERSION 1u

uint32_t aurora_genavb_abi_version(void)
{
    return AURORA_GENAVB_SHIM_ABI_VERSION;
}

void *aurora_genavb_create(void)
{
    return calloc(1, 1);
}

int aurora_genavb_prepare(void *opaque, const void *config)
{
    (void)opaque;
    (void)config;
    return -91;
}

int aurora_genavb_prepare_avdecc(void *opaque, void *control_opaque, uint16_t stream_index,
                                 uint32_t target_latency_frames, uint32_t block_frames)
{
    if (!opaque || !control_opaque)
        return -1;
    if (stream_index != 4 || target_latency_frames != 480 || block_frames != 48)
        return -2;
    return 0;
}

int aurora_genavb_start(void *opaque)
{
    return opaque ? 0 : -1;
}

int aurora_genavb_submit(void *opaque, const uint8_t *payload, uint32_t payload_bytes,
                         uint64_t media_frame_index)
{
    (void)payload;
    (void)payload_bytes;
    (void)media_frame_index;
    return opaque ? 0 : -1;
}

int aurora_genavb_stop(void *opaque)
{
    return opaque ? 0 : -1;
}

int aurora_genavb_reset(void *opaque)
{
    return opaque ? 0 : -1;
}

void aurora_genavb_destroy(void *opaque)
{
    free(opaque);
}

void *aurora_genavb_avdecc_create(void)
{
    return calloc(1, 1);
}

int aurora_genavb_avdecc_open(void *opaque)
{
    return opaque ? 0 : -1;
}

int aurora_genavb_avdecc_rx_fd(void *opaque)
{
    return opaque ? 42 : -1;
}

int aurora_genavb_avdecc_receive(void *opaque, void *event)
{
    (void)event;
    return opaque ? 0 : -1;
}

int aurora_genavb_avdecc_close(void *opaque)
{
    return opaque ? 0 : -1;
}

void aurora_genavb_avdecc_destroy(void *opaque)
{
    free(opaque);
}
