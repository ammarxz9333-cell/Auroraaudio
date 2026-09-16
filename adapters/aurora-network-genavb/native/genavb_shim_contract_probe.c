#include "aurora_genavb_shim.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <genavb/genavb.h>

#define ENDPOINTS 6
#define PAYLOAD_BYTES (48u * 2u * 4u)

static int init_calls;
static int exit_calls;
static int create_calls;
static int destroy_calls;
static int clock_mismatch_index = -1;
static int send_fail_index = -1;
static uint64_t clock_now_ns = 100000000000ULL;
static uint64_t clock_step_ns = 100000ULL;
static unsigned int last_ts[ENDPOINTS];
static unsigned int previous_ts[ENDPOINTS];

int genavb_init(struct genavb_handle **genavb, unsigned int flags)
{
    (void)flags;
    init_calls++;
    *genavb = (struct genavb_handle *)(uintptr_t)0x1;
    return GENAVB_SUCCESS;
}

int genavb_exit(struct genavb_handle *genavb)
{
    if (genavb != (struct genavb_handle *)(uintptr_t)0x1)
        return -1;
    exit_calls++;
    return GENAVB_SUCCESS;
}

int genavb_stream_create(struct genavb_handle *genavb, struct genavb_stream_handle **stream,
                         const struct genavb_stream_params *params, unsigned int *batch_size,
                         genavb_stream_create_flags_t flags)
{
    unsigned int index = (unsigned int)create_calls;

    if (genavb != (struct genavb_handle *)(uintptr_t)0x1 || index >= ENDPOINTS)
        return -1;
    if (params->direction != AVTP_DIRECTION_TALKER || params->subtype != AVTP_SUBTYPE_AAF ||
        params->stream_class != SR_CLASS_B || params->format.u.s.subtype != AVTP_SUBTYPE_AAF ||
        params->format.u.s.subtype_u.aaf.nsr != AAF_NSR_48000 ||
        params->format.u.s.subtype_u.aaf.format != AAF_FORMAT_INT_32BIT ||
        params->format.u.s.subtype_u.aaf.format_u.pcm.bit_depth != 24)
        return -2;
    if (AVDECC_FMT_AAF_PCM_CHANNELS_PER_FRAME(&params->format) != 2)
        return -3;
    if (AVDECC_FMT_AAF_PCM_SAMPLES_PER_FRAME(&params->format) != 24)
        return -4;
    if (*batch_size != PAYLOAD_BYTES || !(flags & AVTP_NONBLOCK))
        return -5;

    *stream = (struct genavb_stream_handle *)(uintptr_t)(0x1000u + index);
    create_calls++;
    return GENAVB_SUCCESS;
}

int genavb_stream_destroy(struct genavb_stream_handle *stream)
{
    if ((uintptr_t)stream < 0x1000u || (uintptr_t)stream >= 0x1000u + ENDPOINTS)
        return -1;
    destroy_calls++;
    return GENAVB_SUCCESS;
}

unsigned int genavb_stream_presentation_offset(const struct genavb_stream_handle *stream)
{
    (void)stream;
    return 2000000u;
}

genavb_clock_id_t genavb_stream_avtp_clock(const struct genavb_stream_handle *stream)
{
    unsigned int index = (unsigned int)((uintptr_t)stream - 0x1000u);

    if ((int)index == clock_mismatch_index)
        return (genavb_clock_id_t)(GENAVB_CLOCK_AVTP_0 + 1);
    return GENAVB_CLOCK_AVTP_0;
}

int genavb_clock_gettime64(genavb_clock_id_t id, uint64_t *ns)
{
    if (id != GENAVB_CLOCK_AVTP_0)
        return -1;
    *ns = clock_now_ns;
    clock_now_ns += clock_step_ns;
    return GENAVB_SUCCESS;
}

int genavb_stream_send(const struct genavb_stream_handle *stream, const void *data,
                       unsigned int data_len, const struct genavb_event *event,
                       unsigned int event_len)
{
    unsigned int index = (unsigned int)((uintptr_t)stream - 0x1000u);

    if (!data || data_len != PAYLOAD_BYTES || index >= ENDPOINTS || event_len != 1)
        return -1;
    if (!event || event->event_mask != AVTP_SYNC || event->index != 0)
        return -2;
    if ((int)index == send_fail_index)
        return -77;
    previous_ts[index] = last_ts[index];
    last_ts[index] = event->ts;
    return (int)data_len;
}

static void reset_stub_state(void)
{
    create_calls = 0;
    destroy_calls = 0;
    clock_mismatch_index = -1;
    send_fail_index = -1;
    clock_now_ns = 100000000000ULL;
    clock_step_ns = 100000ULL;
    memset(last_ts, 0, sizeof(last_ts));
    memset(previous_ts, 0, sizeof(previous_ts));
}

static void fill_config(struct aurora_genavb_config *config, unsigned int id,
                        uint32_t target_latency_frames)
{
    memset(config, 0, sizeof(*config));
    config->port = 0;
    config->stream_id[7] = (uint8_t)id;
    config->destination_mac[0] = 0x91;
    config->destination_mac[1] = 0xe0;
    config->destination_mac[2] = 0xf0;
    config->destination_mac[3] = 0x00;
    config->destination_mac[4] = 0x00;
    config->destination_mac[5] = (uint8_t)id;
    config->target_latency_frames = target_latency_frames;
    config->block_frames = 48;
}

static int all_equal(const unsigned int *values)
{
    unsigned int i;
    for (i = 1; i < ENDPOINTS; i++) {
        if (values[i] != values[0])
            return 0;
    }
    return 1;
}

static int healthy_six_talker_contract(void)
{
    struct aurora_genavb_config config;
    void *handles[ENDPOINTS] = {0};
    uint8_t payload[PAYLOAD_BYTES] = {0};
    unsigned int first_ts;
    unsigned int init_before = (unsigned int)init_calls;
    unsigned int exit_before = (unsigned int)exit_calls;
    unsigned int i;

    reset_stub_state();
    for (i = 0; i < ENDPOINTS; i++) {
        fill_config(&config, i + 1, 480);
        handles[i] = aurora_genavb_create();
        if (!handles[i] || aurora_genavb_prepare(handles[i], &config) != 0 ||
            aurora_genavb_start(handles[i]) != 0)
            return 10;
    }
    if ((unsigned int)init_calls != init_before + 1 || create_calls != ENDPOINTS)
        return 11;

    for (i = 0; i < ENDPOINTS; i++) {
        if (aurora_genavb_submit(handles[i], payload, sizeof(payload), 96000) != 0)
            return 12;
    }
    if (!all_equal(last_ts))
        return 13;
    first_ts = last_ts[0];

    for (i = 0; i < ENDPOINTS; i++) {
        if (aurora_genavb_submit(handles[i], payload, sizeof(payload), 96048) != 0)
            return 14;
    }
    if (!all_equal(last_ts) || !all_equal(previous_ts))
        return 15;
    if ((uint32_t)(last_ts[0] - first_ts) != 1000000u)
        return 16;

    for (i = 0; i < ENDPOINTS; i++) {
        if (aurora_genavb_stop(handles[i]) != 0)
            return 17;
        aurora_genavb_destroy(handles[i]);
    }
    if (destroy_calls != ENDPOINTS || (unsigned int)exit_calls != exit_before + 1)
        return 18;
    return 0;
}

static int mismatched_avtp_clock_fails_closed(void)
{
    struct aurora_genavb_config config;
    void *first;
    void *second;

    reset_stub_state();
    clock_mismatch_index = 1;
    fill_config(&config, 1, 480);
    first = aurora_genavb_create();
    second = aurora_genavb_create();
    if (!first || !second)
        return 20;
    if (aurora_genavb_prepare(first, &config) != 0)
        return 21;
    fill_config(&config, 2, 480);
    if (aurora_genavb_prepare(second, &config) == 0)
        return 22;
    aurora_genavb_destroy(second);
    aurora_genavb_destroy(first);
    return 0;
}

static int mismatched_target_latency_fails_closed(void)
{
    struct aurora_genavb_config config;
    void *first;
    void *second;

    reset_stub_state();
    fill_config(&config, 1, 480);
    first = aurora_genavb_create();
    second = aurora_genavb_create();
    if (!first || !second || aurora_genavb_prepare(first, &config) != 0)
        return 30;
    fill_config(&config, 2, 576);
    if (aurora_genavb_prepare(second, &config) != 0)
        return 31;
    if (aurora_genavb_start(first) != 0)
        return 32;
    if (aurora_genavb_start(second) == 0)
        return 33;
    if (aurora_genavb_stop(first) != 0)
        return 34;
    aurora_genavb_destroy(second);
    aurora_genavb_destroy(first);
    return 0;
}

static int late_presentation_fails_closed(void)
{
    struct aurora_genavb_config config;
    uint8_t payload[PAYLOAD_BYTES] = {0};
    void *handle;

    reset_stub_state();
    clock_step_ns = 9000000ULL;
    fill_config(&config, 1, 480);
    handle = aurora_genavb_create();
    if (!handle || aurora_genavb_prepare(handle, &config) != 0 ||
        aurora_genavb_start(handle) != 0)
        return 40;
    if (aurora_genavb_submit(handle, payload, sizeof(payload), 96000) == 0)
        return 41;
    if (aurora_genavb_stop(handle) != 0)
        return 42;
    aurora_genavb_destroy(handle);
    return 0;
}

static int send_failure_propagates(void)
{
    struct aurora_genavb_config config;
    uint8_t payload[PAYLOAD_BYTES] = {0};
    void *handle;

    reset_stub_state();
    send_fail_index = 0;
    fill_config(&config, 1, 480);
    handle = aurora_genavb_create();
    if (!handle || aurora_genavb_prepare(handle, &config) != 0 ||
        aurora_genavb_start(handle) != 0)
        return 50;
    if (aurora_genavb_submit(handle, payload, sizeof(payload), 96000) != -77)
        return 51;
    if (aurora_genavb_stop(handle) != 0)
        return 52;
    aurora_genavb_destroy(handle);
    return 0;
}

int main(void)
{
    int rc;

    if (aurora_genavb_abi_version() != AURORA_GENAVB_SHIM_ABI_VERSION)
        return 1;

    rc = healthy_six_talker_contract();
    if (rc)
        return rc;
    rc = mismatched_avtp_clock_fails_closed();
    if (rc)
        return rc;
    rc = mismatched_target_latency_fails_closed();
    if (rc)
        return rc;
    rc = late_presentation_fails_closed();
    if (rc)
        return rc;
    rc = send_failure_propagates();
    if (rc)
        return rc;

    printf("aurora-genavb-shim: PASS talkers=6 init_calls=1 rate=48000 channels=2 block=48 shared_ptp_anchor=1 delta_ns=1000000 clock_mismatch=fail-closed latency_mismatch=fail-closed late_deadline=fail-closed send_failure=propagated\n");
    return 0;
}
