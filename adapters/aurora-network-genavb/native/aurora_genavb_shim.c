#include "aurora_genavb_shim.h"

#include <limits.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <genavb/genavb.h>
#include <genavb/aaf.h>
#include <genavb/avdecc.h>
#include <genavb/avtp.h>

#define AURORA_GENAVB_RATE_HZ 48000u
#define AURORA_GENAVB_CHANNELS 2u
#define AURORA_GENAVB_BLOCK_FRAMES 48u
#define AURORA_GENAVB_BYTES_PER_SAMPLE 4u
#define AURORA_GENAVB_PAYLOAD_BYTES \
    (AURORA_GENAVB_BLOCK_FRAMES * AURORA_GENAVB_CHANNELS * AURORA_GENAVB_BYTES_PER_SAMPLE)
#define AURORA_GENAVB_BATCH_NS 1000000u

struct aurora_genavb_handle {
    struct genavb_stream_handle *stream;
    genavb_clock_id_t avtp_clock;
    uint32_t presentation_offset_ns;
    uint64_t target_latency_ns;
    uint32_t block_frames;
    int runtime_acquired;
    int prepared;
    int started;
};

struct aurora_genavb_avdecc_handle {
    struct genavb_control_handle *control;
    int runtime_acquired;
};

static pthread_mutex_t runtime_mutex = PTHREAD_MUTEX_INITIALIZER;
static struct genavb_handle *runtime_genavb;
static unsigned int runtime_users;
static int runtime_clock_valid;
static genavb_clock_id_t runtime_avtp_clock;

static pthread_mutex_t anchor_mutex = PTHREAD_MUTEX_INITIALIZER;
static unsigned int started_users;
static int anchor_valid;
static uint64_t anchor_media_frame;
static uint64_t anchor_avtp_ns;
static uint64_t started_target_latency_ns;

static int runtime_acquire(struct genavb_handle **genavb)
{
    int rc = 0;

    if (pthread_mutex_lock(&runtime_mutex) != 0)
        return -1;

    if (!runtime_genavb) {
        rc = genavb_init(&runtime_genavb, 0);
        if (rc != GENAVB_SUCCESS) {
            runtime_genavb = NULL;
            pthread_mutex_unlock(&runtime_mutex);
            return rc;
        }
    }

    runtime_users++;
    *genavb = runtime_genavb;
    pthread_mutex_unlock(&runtime_mutex);
    return 0;
}

static int runtime_register_clock(genavb_clock_id_t clock_id)
{
    int rc = 0;

    if (pthread_mutex_lock(&runtime_mutex) != 0)
        return -1;

    if (!runtime_clock_valid) {
        runtime_avtp_clock = clock_id;
        runtime_clock_valid = 1;
    } else if (runtime_avtp_clock != clock_id) {
        rc = -1;
    }

    pthread_mutex_unlock(&runtime_mutex);
    return rc;
}

static void runtime_release(void)
{
    if (pthread_mutex_lock(&runtime_mutex) != 0)
        return;

    if (runtime_users > 0)
        runtime_users--;

    if ((runtime_users == 0) && runtime_genavb) {
        struct genavb_handle *genavb = runtime_genavb;
        runtime_genavb = NULL;
        runtime_clock_valid = 0;
        genavb_exit(genavb);
    }

    pthread_mutex_unlock(&runtime_mutex);
}

static int anchor_start(struct aurora_genavb_handle *handle)
{
    int rc = 0;

    if (pthread_mutex_lock(&anchor_mutex) != 0)
        return -1;

    if (started_users == 0) {
        anchor_valid = 0;
        started_target_latency_ns = handle->target_latency_ns;
    } else if (started_target_latency_ns != handle->target_latency_ns) {
        rc = -1;
        goto out;
    }

    started_users++;
out:
    pthread_mutex_unlock(&anchor_mutex);
    return rc;
}

static void anchor_stop(struct aurora_genavb_handle *handle)
{
    if (!handle->started)
        return;

    if (pthread_mutex_lock(&anchor_mutex) != 0)
        return;

    if (started_users > 0)
        started_users--;
    if (started_users == 0) {
        anchor_valid = 0;
        started_target_latency_ns = 0;
    }

    pthread_mutex_unlock(&anchor_mutex);
    handle->started = 0;
}

static int anchor_presentation_time(struct aurora_genavb_handle *handle,
                                    uint64_t media_frame_index,
                                    uint64_t *presentation_ns)
{
    uint64_t now_ns;
    uint64_t delta_frames;
    uint64_t delta_ns;
    int rc = 0;

    if (pthread_mutex_lock(&anchor_mutex) != 0)
        return -1;

    if (!anchor_valid) {
        rc = genavb_clock_gettime64(handle->avtp_clock, &now_ns);
        if (rc != GENAVB_SUCCESS)
            goto out;
        if (UINT64_MAX - now_ns < handle->target_latency_ns) {
            rc = -2;
            goto out;
        }
        anchor_media_frame = media_frame_index;
        anchor_avtp_ns = now_ns + handle->target_latency_ns;
        anchor_valid = 1;
    }

    if (media_frame_index < anchor_media_frame) {
        rc = -3;
        goto out;
    }
    delta_frames = media_frame_index - anchor_media_frame;
    if ((delta_frames % handle->block_frames) != 0) {
        rc = -4;
        goto out;
    }
    if (delta_frames > (UINT64_MAX / 1000000000ULL)) {
        rc = -5;
        goto out;
    }
    delta_ns = (delta_frames * 1000000000ULL) / AURORA_GENAVB_RATE_HZ;
    if (UINT64_MAX - anchor_avtp_ns < delta_ns) {
        rc = -6;
        goto out;
    }
    *presentation_ns = anchor_avtp_ns + delta_ns;

out:
    pthread_mutex_unlock(&anchor_mutex);
    return rc;
}

static void stream_cleanup(struct aurora_genavb_handle *handle)
{
    anchor_stop(handle);
    if (handle->stream) {
        genavb_stream_destroy(handle->stream);
        handle->stream = NULL;
    }
    if (handle->runtime_acquired) {
        runtime_release();
        handle->runtime_acquired = 0;
    }
    handle->prepared = 0;
    handle->started = 0;
}

static int frames_to_ns(uint32_t frames, uint64_t *ns)
{
    uint64_t numerator = (uint64_t)frames * 1000000000ULL;

    if (!frames || (numerator % AURORA_GENAVB_RATE_HZ) != 0)
        return -1;

    *ns = numerator / AURORA_GENAVB_RATE_HZ;
    return 0;
}

uint32_t aurora_genavb_abi_version(void)
{
    return AURORA_GENAVB_SHIM_ABI_VERSION;
}

void *aurora_genavb_create(void)
{
    return calloc(1, sizeof(struct aurora_genavb_handle));
}

int aurora_genavb_prepare(void *opaque, const struct aurora_genavb_config *config)
{
    struct aurora_genavb_handle *handle = opaque;
    struct genavb_handle *genavb = NULL;
    struct genavb_stream_params params;
    unsigned int batch_size = AURORA_GENAVB_PAYLOAD_BYTES;
    uint64_t target_latency_ns;
    int rc;

    if (!handle || !config || config->block_frames != AURORA_GENAVB_BLOCK_FRAMES)
        return -1;
    if (frames_to_ns(config->target_latency_frames, &target_latency_ns) < 0)
        return -2;

    stream_cleanup(handle);

    rc = runtime_acquire(&genavb);
    if (rc)
        return rc;
    handle->runtime_acquired = 1;

    memset(&params, 0, sizeof(params));
    params.direction = AVTP_DIRECTION_TALKER;
    params.subtype = AVTP_SUBTYPE_AAF;
    params.port = config->port;
    params.stream_class = SR_CLASS_B;
    memcpy(params.stream_id, config->stream_id, sizeof(params.stream_id));
    memcpy(params.dst_mac, config->destination_mac, sizeof(params.dst_mac));
    params.clock_domain = AVB_CLOCK_DOMAIN_0;
    params.flags = 0;
    params.talker.latency = AURORA_GENAVB_BATCH_NS;

    params.format.u.s.v = 0;
    params.format.u.s.subtype = AVTP_SUBTYPE_AAF;
    params.format.u.s.subtype_u.aaf.nsr = AAF_NSR_48000;
    params.format.u.s.subtype_u.aaf.ut = 0;
    params.format.u.s.subtype_u.aaf.rsvd = 0;
    params.format.u.s.subtype_u.aaf.format = AAF_FORMAT_INT_32BIT;
    params.format.u.s.subtype_u.aaf.format_u.pcm.bit_depth = 24;
    AVDECC_FMT_AAF_PCM_CHANNELS_PER_FRAME_SET(&params.format, AURORA_GENAVB_CHANNELS);
    AVDECC_FMT_AAF_PCM_SAMPLES_PER_FRAME_SET(&params.format, 24);

    rc = genavb_stream_create(genavb, &handle->stream, &params, &batch_size, AVTP_NONBLOCK);
    if (rc != GENAVB_SUCCESS)
        goto fail;

    handle->presentation_offset_ns = genavb_stream_presentation_offset(handle->stream);
    handle->avtp_clock = genavb_stream_avtp_clock(handle->stream);
    handle->target_latency_ns = target_latency_ns;
    handle->block_frames = config->block_frames;

    if (runtime_register_clock(handle->avtp_clock) < 0) {
        rc = -3;
        goto fail;
    }
    if (handle->target_latency_ns < handle->presentation_offset_ns) {
        rc = -4;
        goto fail;
    }

    handle->prepared = 1;
    return 0;

fail:
    stream_cleanup(handle);
    return rc;
}

int aurora_genavb_start(void *opaque)
{
    struct aurora_genavb_handle *handle = opaque;

    if (!handle || !handle->prepared || handle->started)
        return -1;
    if (anchor_start(handle) < 0)
        return -2;

    handle->started = 1;
    return 0;
}

int aurora_genavb_submit(void *opaque, const uint8_t *aaf_payload, uint32_t payload_bytes,
                         uint64_t media_frame_index)
{
    struct aurora_genavb_handle *handle = opaque;
    struct genavb_event event;
    uint64_t now_ns;
    uint64_t presentation_ns;
    int rc;

    if (!handle || !handle->started || !handle->stream || !aaf_payload)
        return -1;
    if (payload_bytes != AURORA_GENAVB_PAYLOAD_BYTES)
        return -2;

    rc = anchor_presentation_time(handle, media_frame_index, &presentation_ns);
    if (rc)
        return rc;

    rc = genavb_clock_gettime64(handle->avtp_clock, &now_ns);
    if (rc != GENAVB_SUCCESS)
        return rc;
    if ((UINT64_MAX - now_ns < handle->presentation_offset_ns) ||
        (presentation_ns < now_ns + handle->presentation_offset_ns))
        return -8;

    memset(&event, 0, sizeof(event));
    event.event_mask = AVTP_SYNC;
    event.index = 0;
    event.ts = (uint32_t)presentation_ns;

    rc = genavb_stream_send(handle->stream, aaf_payload, payload_bytes, &event, 1);
    if (rc != (int)payload_bytes)
        return (rc < 0) ? rc : -9;

    return 0;
}

int aurora_genavb_stop(void *opaque)
{
    struct aurora_genavb_handle *handle = opaque;

    if (!handle || !handle->prepared)
        return -1;

    anchor_stop(handle);
    return 0;
}

int aurora_genavb_reset(void *opaque)
{
    struct aurora_genavb_handle *handle = opaque;

    if (!handle || handle->started)
        return -1;

    return 0;
}

void aurora_genavb_destroy(void *opaque)
{
    struct aurora_genavb_handle *handle = opaque;

    if (!handle)
        return;

    stream_cleanup(handle);
    free(handle);
}

static int avdecc_connect_is_supported(const struct genavb_msg_media_stack_connect *connect)
{
    const struct genavb_stream_params *params = &connect->stream_params;

    if (params->direction != AVTP_DIRECTION_TALKER || params->subtype != AVTP_SUBTYPE_AAF)
        return 0;
    if (!avdecc_format_is_aaf_pcm(&params->format))
        return 0;
    if (params->format.u.s.subtype_u.aaf.nsr != AAF_NSR_48000 ||
        params->format.u.s.subtype_u.aaf.format != AAF_FORMAT_INT_32BIT ||
        params->format.u.s.subtype_u.aaf.format_u.pcm.bit_depth != 24)
        return 0;
    if (AVDECC_FMT_AAF_PCM_CHANNELS_PER_FRAME(&params->format) != AURORA_GENAVB_CHANNELS)
        return 0;

    return 1;
}

void *aurora_genavb_avdecc_create(void)
{
    return calloc(1, sizeof(struct aurora_genavb_avdecc_handle));
}

int aurora_genavb_avdecc_open(void *opaque)
{
    struct aurora_genavb_avdecc_handle *handle = opaque;
    struct genavb_handle *genavb = NULL;
    int rc;

    if (!handle || handle->control || handle->runtime_acquired)
        return -1;

    rc = runtime_acquire(&genavb);
    if (rc)
        return rc;
    handle->runtime_acquired = 1;

    rc = genavb_control_open(genavb, &handle->control, GENAVB_CTRL_AVDECC_MEDIA_STACK);
    if (rc != GENAVB_SUCCESS) {
        runtime_release();
        handle->runtime_acquired = 0;
        handle->control = NULL;
        return rc;
    }

    return 0;
}

int aurora_genavb_avdecc_rx_fd(void *opaque)
{
    struct aurora_genavb_avdecc_handle *handle = opaque;

    if (!handle || !handle->control)
        return -1;

    return genavb_control_rx_fd(handle->control);
}

int aurora_genavb_avdecc_receive(void *opaque, struct aurora_genavb_avdecc_event *event)
{
    struct aurora_genavb_avdecc_handle *handle = opaque;
    union genavb_media_stack_msg message;
    genavb_msg_type_t message_type;
    unsigned int message_len = sizeof(message);
    int rc;

    if (!handle || !handle->control || !event)
        return -1;

    memset(event, 0, sizeof(*event));
    memset(&message, 0, sizeof(message));

    rc = genavb_control_receive(handle->control, &message_type, &message, &message_len);
    if (rc != GENAVB_SUCCESS)
        return rc;

    switch (message_type) {
    case GENAVB_MSG_MEDIA_STACK_CONNECT: {
        const struct genavb_msg_media_stack_connect *connect = &message.media_stack_connect;
        const struct genavb_stream_params *params = &connect->stream_params;

        if (!avdecc_connect_is_supported(connect))
            return -2;

        event->kind = AURORA_GENAVB_AVDECC_EVENT_CONNECT;
        event->stream_index = connect->stream_index;
        event->port = params->port;
        event->direction = (uint16_t)params->direction;
        event->stream_class = (uint16_t)params->stream_class;
        memcpy(event->stream_id, params->stream_id, sizeof(event->stream_id));
        memcpy(event->destination_mac, params->dst_mac, sizeof(event->destination_mac));
        event->sample_rate_hz = AURORA_GENAVB_RATE_HZ;
        event->channels = AURORA_GENAVB_CHANNELS;
        event->bit_depth = 24;
        return 1;
    }
    case GENAVB_MSG_MEDIA_STACK_DISCONNECT:
        if (message.media_stack_disconnect.direction != AVTP_DIRECTION_TALKER)
            return -3;
        event->kind = AURORA_GENAVB_AVDECC_EVENT_DISCONNECT;
        event->stream_index = message.media_stack_disconnect.stream_index;
        event->port = message.media_stack_disconnect.port;
        event->direction = (uint16_t)message.media_stack_disconnect.direction;
        event->stream_class = (uint16_t)message.media_stack_disconnect.stream_class;
        memcpy(event->stream_id, message.media_stack_disconnect.stream_id, sizeof(event->stream_id));
        return 1;
    case GENAVB_MSG_MEDIA_STACK_BIND:
    case GENAVB_MSG_MEDIA_STACK_UNBIND:
    case GENAVB_MSG_MEDIA_SET_CLOCK_SOURCE:
    case GENAVB_MSG_MEDIA_STACK_PERSISTENT_PARAM:
        return 0;
    default:
        return -4;
    }
}

int aurora_genavb_avdecc_close(void *opaque)
{
    struct aurora_genavb_avdecc_handle *handle = opaque;
    int rc = 0;

    if (!handle)
        return -1;

    if (handle->control) {
        rc = genavb_control_close(handle->control);
        handle->control = NULL;
    }
    if (handle->runtime_acquired) {
        runtime_release();
        handle->runtime_acquired = 0;
    }

    return rc;
}

void aurora_genavb_avdecc_destroy(void *opaque)
{
    struct aurora_genavb_avdecc_handle *handle = opaque;

    if (!handle)
        return;

    aurora_genavb_avdecc_close(handle);
    free(handle);
}
