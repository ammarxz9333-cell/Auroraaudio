#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define AURORA_GENAVB_SHIM_ABI_VERSION 1u

struct mock_talker {
    uint16_t stream_index;
    int prepared;
    int started;
};

struct mock_control {
    unsigned int receive_count;
    int open;
};

struct mock_avdecc_event {
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

static int env_matches_stream(const char *name, uint16_t stream_index)
{
    const char *value = getenv(name);
    char *end = NULL;
    long parsed;

    if (!value || !*value)
        return 0;
    parsed = strtol(value, &end, 10);
    return end && *end == '\0' && parsed >= 0 && parsed <= UINT16_MAX &&
           (uint16_t)parsed == stream_index;
}

static void fill_event(struct mock_avdecc_event *event, uint32_t kind, uint16_t stream_index,
                       uint8_t identity)
{
    memset(event, 0, sizeof(*event));
    event->kind = kind;
    event->stream_index = stream_index;
    event->port = 0;
    event->direction = 0;
    event->stream_class = 1;
    event->stream_id[0] = 0x10;
    event->stream_id[7] = identity;
    event->destination_mac[0] = 0x91;
    event->destination_mac[1] = 0xe0;
    event->destination_mac[2] = 0xf0;
    event->destination_mac[5] = identity;
    if (kind == 1) {
        event->sample_rate_hz = 48000;
        event->channels = 2;
        event->bit_depth = 24;
    }
}

uint32_t aurora_genavb_abi_version(void)
{
    return AURORA_GENAVB_SHIM_ABI_VERSION;
}

void *aurora_genavb_create(void)
{
    return calloc(1, sizeof(struct mock_talker));
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
    struct mock_talker *talker = opaque;
    struct mock_control *control = control_opaque;

    if (!talker || !control || !control->open)
        return -1;
    if (!((stream_index == 4) || (stream_index >= 10 && stream_index <= 15)) ||
        target_latency_frames != 480 || block_frames != 48)
        return -2;
    if (env_matches_stream("AURORA_GENAVB_MOCK_FAIL_PREPARE", stream_index))
        return -3;

    talker->stream_index = stream_index;
    talker->prepared = 1;
    talker->started = 0;
    return 0;
}

int aurora_genavb_start(void *opaque)
{
    struct mock_talker *talker = opaque;

    if (!talker || !talker->prepared)
        return -1;
    if (env_matches_stream("AURORA_GENAVB_MOCK_FAIL_START", talker->stream_index))
        return -2;
    talker->started = 1;
    return 0;
}

int aurora_genavb_submit(void *opaque, const uint8_t *payload, uint32_t payload_bytes,
                         uint64_t media_frame_index)
{
    struct mock_talker *talker = opaque;

    (void)media_frame_index;
    if (!talker || !talker->started || !payload || payload_bytes != 384)
        return -1;
    if (env_matches_stream("AURORA_GENAVB_MOCK_FAIL_SUBMIT", talker->stream_index))
        return -2;
    return 0;
}

int aurora_genavb_stop(void *opaque)
{
    struct mock_talker *talker = opaque;

    if (!talker || !talker->prepared)
        return -1;
    talker->started = 0;
    return 0;
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
    return calloc(1, sizeof(struct mock_control));
}

int aurora_genavb_avdecc_open(void *opaque)
{
    struct mock_control *control = opaque;

    if (!control)
        return -1;
    control->receive_count = 0;
    control->open = 1;
    return 0;
}

int aurora_genavb_avdecc_rx_fd(void *opaque)
{
    struct mock_control *control = opaque;
    return (control && control->open) ? 42 : -1;
}

int aurora_genavb_avdecc_receive(void *opaque, void *event_opaque)
{
    struct mock_control *control = opaque;
    struct mock_avdecc_event *event = event_opaque;
    unsigned int index;

    if (!control || !control->open || !event)
        return -1;

    index = control->receive_count++;
    if (index < 6) {
        fill_event(event, 1, (uint16_t)(10 + index), (uint8_t)(1 + index));
        return 1;
    }

    if (getenv("AURORA_GENAVB_MOCK_DISCONNECT_RECONNECT")) {
        if (index == 6) {
            fill_event(event, 2, 13, 4);
            return 1;
        }
        if (index == 7) {
            fill_event(event, 1, 13, 4);
            return 1;
        }
    }

    memset(event, 0, sizeof(*event));
    return 0;
}

int aurora_genavb_avdecc_close(void *opaque)
{
    struct mock_control *control = opaque;

    if (!control)
        return -1;
    control->open = 0;
    control->receive_count = 0;
    return 0;
}

void aurora_genavb_avdecc_destroy(void *opaque)
{
    free(opaque);
}
