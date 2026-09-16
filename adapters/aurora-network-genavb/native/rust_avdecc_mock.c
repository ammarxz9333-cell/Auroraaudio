#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define AURORA_GENAVB_SHIM_ABI_VERSION 1u
#define MOCK_CONNECT 1u
#define MOCK_DISCONNECT 2u
#define MOCK_RUNTIME_STREAMS 6u

struct mock_control {
    unsigned int cursor;
};

struct mock_event {
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

static unsigned int runtime_prepare_calls;
static unsigned int runtime_start_calls;
static unsigned int runtime_submit_calls;
static unsigned int runtime_stop_calls;
static unsigned int runtime_reset_calls;

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
    uint16_t expected;

    if (!opaque || !control_opaque)
        return -1;
    if (target_latency_frames != 480 || block_frames != 48)
        return -2;

    /* Preserve the pre-existing single-stream FFI probe. */
    if (stream_index == 4)
        return 0;

    if (runtime_prepare_calls >= MOCK_RUNTIME_STREAMS)
        return -3;
    expected = (uint16_t)(10u + runtime_prepare_calls);
    if (stream_index != expected)
        return -4;

    runtime_prepare_calls++;
    return 0;
}

int aurora_genavb_start(void *opaque)
{
    if (!opaque || runtime_prepare_calls != MOCK_RUNTIME_STREAMS ||
        runtime_start_calls >= MOCK_RUNTIME_STREAMS)
        return -1;
    runtime_start_calls++;
    return 0;
}

int aurora_genavb_submit(void *opaque, const uint8_t *payload, uint32_t payload_bytes,
                         uint64_t media_frame_index)
{
    if (!opaque || !payload)
        return -1;
    if (runtime_start_calls != MOCK_RUNTIME_STREAMS ||
        runtime_submit_calls >= MOCK_RUNTIME_STREAMS)
        return -2;
    if (payload_bytes != 384u || media_frame_index != 96000u)
        return -3;
    runtime_submit_calls++;
    return 0;
}

int aurora_genavb_stop(void *opaque)
{
    if (!opaque)
        return -1;
    if (runtime_start_calls == MOCK_RUNTIME_STREAMS &&
        runtime_stop_calls < MOCK_RUNTIME_STREAMS)
        runtime_stop_calls++;
    return 0;
}

int aurora_genavb_reset(void *opaque)
{
    if (!opaque)
        return -1;
    if (runtime_prepare_calls == MOCK_RUNTIME_STREAMS)
        runtime_reset_calls++;
    return 0;
}

void aurora_genavb_destroy(void *opaque)
{
    free(opaque);
}

void *aurora_genavb_avdecc_create(void)
{
    return calloc(1, sizeof(struct mock_control));
}

int aurora_genavb_avdecc_open(void *opaque)
{
    return opaque ? 0 : -1;
}

int aurora_genavb_avdecc_rx_fd(void *opaque)
{
    return opaque ? 42 : -1;
}

int aurora_genavb_avdecc_receive(void *opaque, void *event_opaque)
{
    struct mock_control *control = opaque;
    struct mock_event *event = event_opaque;
    unsigned int identity;

    if (!control || !event)
        return -1;
    memset(event, 0, sizeof(*event));

    if (control->cursor < MOCK_RUNTIME_STREAMS) {
        identity = control->cursor + 1u;
        event->kind = MOCK_CONNECT;
        event->stream_index = (uint16_t)(10u + control->cursor);
        event->port = 0;
        event->direction = 0;
        event->stream_class = 1;
        event->stream_id[0] = 0;
        event->stream_id[1] = 1;
        event->stream_id[2] = 2;
        event->stream_id[3] = 3;
        event->stream_id[4] = 4;
        event->stream_id[5] = 5;
        event->stream_id[6] = 6;
        event->stream_id[7] = (uint8_t)identity;
        event->destination_mac[0] = 0x91;
        event->destination_mac[1] = 0xe0;
        event->destination_mac[2] = 0xf0;
        event->destination_mac[3] = 0;
        event->destination_mac[4] = 0;
        event->destination_mac[5] = (uint8_t)identity;
        event->sample_rate_hz = 48000;
        event->channels = 2;
        event->bit_depth = 24;
        control->cursor++;
        return 1;
    }

    if (control->cursor == MOCK_RUNTIME_STREAMS) {
        event->kind = MOCK_DISCONNECT;
        event->stream_index = 12;
        event->port = 0;
        event->direction = 0;
        event->stream_class = 1;
        event->stream_id[0] = 0;
        event->stream_id[1] = 1;
        event->stream_id[2] = 2;
        event->stream_id[3] = 3;
        event->stream_id[4] = 4;
        event->stream_id[5] = 5;
        event->stream_id[6] = 6;
        event->stream_id[7] = 3;
        control->cursor++;
        return 1;
    }

    return 0;
}

int aurora_genavb_avdecc_close(void *opaque)
{
    struct mock_control *control = opaque;

    if (!control)
        return -1;
    if (control->cursor > 0 &&
        (runtime_prepare_calls != MOCK_RUNTIME_STREAMS ||
         runtime_start_calls != MOCK_RUNTIME_STREAMS ||
         runtime_submit_calls != MOCK_RUNTIME_STREAMS ||
         runtime_stop_calls != MOCK_RUNTIME_STREAMS ||
         runtime_reset_calls < MOCK_RUNTIME_STREAMS))
        return -10;
    return 0;
}

void aurora_genavb_avdecc_destroy(void *opaque)
{
    free(opaque);
}
