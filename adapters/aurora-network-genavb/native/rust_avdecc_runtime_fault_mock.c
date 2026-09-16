#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define AURORA_GENAVB_SHIM_ABI_VERSION 1u
#define MOCK_CONNECT 1u
#define MOCK_DISCONNECT 2u
#define MOCK_STREAMS 6u

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

static unsigned int prepare_attempts;
static unsigned int prepare_successes;
static unsigned int start_attempts;
static unsigned int start_successes;
static unsigned int submit_attempts;
static unsigned int submit_successes;
static unsigned int stop_calls;
static unsigned int reset_calls;

static const char *mode(void)
{
    const char *value = getenv("AURORA_GENAVB_MOCK_MODE");
    return value ? value : "";
}

static int is_mode(const char *value)
{
    return strcmp(mode(), value) == 0;
}

static void fill_connect(struct mock_event *event, uint16_t stream_index, uint8_t identity)
{
    memset(event, 0, sizeof(*event));
    event->kind = MOCK_CONNECT;
    event->stream_index = stream_index;
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
    event->stream_id[7] = identity;
    event->destination_mac[0] = 0x91;
    event->destination_mac[1] = 0xe0;
    event->destination_mac[2] = 0xf0;
    event->destination_mac[3] = 0;
    event->destination_mac[4] = 0;
    event->destination_mac[5] = identity;
    event->sample_rate_hz = 48000;
    event->channels = 2;
    event->bit_depth = 24;
}

static void fill_disconnect(struct mock_event *event, uint16_t stream_index, uint8_t identity)
{
    fill_connect(event, stream_index, identity);
    event->kind = MOCK_DISCONNECT;
    event->destination_mac[0] = 0;
    event->destination_mac[1] = 0;
    event->destination_mac[2] = 0;
    event->destination_mac[3] = 0;
    event->destination_mac[4] = 0;
    event->destination_mac[5] = 0;
    event->sample_rate_hz = 0;
    event->channels = 0;
    event->bit_depth = 0;
}

static int validate_close_counts(void)
{
    if (is_mode("prepare-fail"))
        return prepare_successes == 2 && start_successes == 0 && submit_successes == 0 &&
               stop_calls == 0 && reset_calls >= 2;
    if (is_mode("start-fail"))
        return prepare_successes == 6 && start_successes == 2 && submit_successes == 0 &&
               stop_calls == 2 && reset_calls >= 6;
    if (is_mode("submit-fail"))
        return prepare_successes == 6 && start_successes == 6 && submit_successes == 2 &&
               stop_calls == 6 && reset_calls >= 6;
    if (is_mode("duplicate-connect"))
        return prepare_successes == 0 && start_successes == 0 && submit_successes == 0 &&
               stop_calls == 0 && reset_calls == 0;
    if (is_mode("reordered"))
        return prepare_successes == 6 && start_successes == 6 && submit_successes == 6 &&
               stop_calls == 6 && reset_calls >= 6;
    if (is_mode("reconnect"))
        return prepare_successes == 12 && start_successes == 12 && submit_successes == 12 &&
               stop_calls == 12 && reset_calls >= 12;
    return 0;
}

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
    unsigned int attempt;

    if (!opaque || !control_opaque)
        return -1;
    if (target_latency_frames != 480 || block_frames != 48)
        return -2;

    attempt = prepare_attempts++;
    expected = (uint16_t)(10u + (attempt % MOCK_STREAMS));
    if (stream_index != expected)
        return -3;
    if (is_mode("prepare-fail") && attempt == 2u)
        return -41;

    prepare_successes++;
    return 0;
}

int aurora_genavb_start(void *opaque)
{
    unsigned int attempt;

    if (!opaque)
        return -1;
    attempt = start_attempts++;
    if (is_mode("start-fail") && attempt == 2u)
        return -42;
    start_successes++;
    return 0;
}

int aurora_genavb_submit(void *opaque, const uint8_t *payload, uint32_t payload_bytes,
                         uint64_t media_frame_index)
{
    unsigned int attempt;
    unsigned int epoch;
    uint64_t expected_frame;

    if (!opaque || !payload || payload_bytes != 384u)
        return -1;
    attempt = submit_attempts++;
    if (is_mode("submit-fail") && attempt == 2u)
        return -43;

    epoch = attempt / MOCK_STREAMS;
    expected_frame = epoch == 0u ? 96000u : 96048u;
    if (media_frame_index != expected_frame)
        return -2;

    submit_successes++;
    return 0;
}

int aurora_genavb_stop(void *opaque)
{
    if (!opaque)
        return -1;
    stop_calls++;
    return 0;
}

int aurora_genavb_reset(void *opaque)
{
    if (!opaque)
        return -1;
    reset_calls++;
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
    static const uint16_t reordered[MOCK_STREAMS] = {15, 10, 13, 11, 14, 12};
    struct mock_control *control = opaque;
    struct mock_event *event = event_opaque;
    uint16_t stream_index;
    uint8_t identity;

    if (!control || !event)
        return -1;

    if (is_mode("duplicate-connect")) {
        if (control->cursor == 0u) {
            fill_connect(event, 10, 1);
            control->cursor++;
            return 1;
        }
        if (control->cursor == 1u) {
            fill_connect(event, 10, 2);
            control->cursor++;
            return 1;
        }
        return 0;
    }

    if (control->cursor < MOCK_STREAMS) {
        if (is_mode("reordered"))
            stream_index = reordered[control->cursor];
        else
            stream_index = (uint16_t)(10u + control->cursor);
        identity = (uint8_t)(stream_index - 9u);
        fill_connect(event, stream_index, identity);
        control->cursor++;
        return 1;
    }

    if (is_mode("reconnect")) {
        if (control->cursor == MOCK_STREAMS) {
            fill_disconnect(event, 12, 3);
            control->cursor++;
            return 1;
        }
        if (control->cursor == MOCK_STREAMS + 1u) {
            fill_connect(event, 12, 33);
            control->cursor++;
            return 1;
        }
    }

    return 0;
}

int aurora_genavb_avdecc_close(void *opaque)
{
    if (!opaque)
        return -1;
    return validate_close_counts() ? 0 : -10;
}

void aurora_genavb_avdecc_destroy(void *opaque)
{
    free(opaque);
}
