#include "aurora_genavb_shim.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <genavb/genavb.h>

static int init_calls;
static int exit_calls;
static int control_open_calls;
static int control_close_calls;
static int receive_case;

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

int genavb_control_open(const struct genavb_handle *genavb, struct genavb_control_handle **handle,
                        genavb_control_id_t id)
{
    if (genavb != (struct genavb_handle *)(uintptr_t)0x1 ||
        id != GENAVB_CTRL_AVDECC_MEDIA_STACK)
        return -1;
    control_open_calls++;
    *handle = (struct genavb_control_handle *)(uintptr_t)0x2000u;
    return GENAVB_SUCCESS;
}

int genavb_control_close(struct genavb_control_handle *handle)
{
    if (handle != (struct genavb_control_handle *)(uintptr_t)0x2000u)
        return -1;
    control_close_calls++;
    return GENAVB_SUCCESS;
}

int genavb_control_rx_fd(const struct genavb_control_handle *handle)
{
    return (handle == (struct genavb_control_handle *)(uintptr_t)0x2000u) ? 42 : -1;
}

static void fill_connect(struct genavb_msg_media_stack_connect *connect, unsigned int bit_depth)
{
    struct genavb_stream_params *params = &connect->stream_params;

    memset(connect, 0, sizeof(*connect));
    connect->stream_index = 4;
    params->direction = AVTP_DIRECTION_TALKER;
    params->subtype = AVTP_SUBTYPE_AAF;
    params->port = 0;
    params->stream_class = SR_CLASS_B;
    params->stream_id[0] = 0x10;
    params->stream_id[7] = 0x04;
    params->dst_mac[0] = 0x91;
    params->dst_mac[1] = 0xe0;
    params->dst_mac[2] = 0xf0;
    params->dst_mac[5] = 0x04;
    params->format.u.s.v = 0;
    params->format.u.s.subtype = AVTP_SUBTYPE_AAF;
    params->format.u.s.subtype_u.aaf.nsr = AAF_NSR_48000;
    params->format.u.s.subtype_u.aaf.format = AAF_FORMAT_INT_32BIT;
    params->format.u.s.subtype_u.aaf.format_u.pcm.bit_depth = bit_depth;
    AVDECC_FMT_AAF_PCM_CHANNELS_PER_FRAME_SET(&params->format, 2);
    AVDECC_FMT_AAF_PCM_SAMPLES_PER_FRAME_SET(&params->format, 24);
}

int genavb_control_receive(const struct genavb_control_handle *handle, genavb_msg_type_t *msg_type,
                           void *msg, unsigned int *msg_len)
{
    union genavb_media_stack_msg *media = msg;

    if (handle != (struct genavb_control_handle *)(uintptr_t)0x2000u ||
        !msg_type || !msg || !msg_len || *msg_len < sizeof(*media))
        return -1;

    memset(media, 0, sizeof(*media));
    switch (receive_case) {
    case 0:
        *msg_type = GENAVB_MSG_MEDIA_STACK_CONNECT;
        fill_connect(&media->media_stack_connect, 24);
        break;
    case 1:
        *msg_type = GENAVB_MSG_MEDIA_STACK_CONNECT;
        fill_connect(&media->media_stack_connect, 16);
        break;
    case 2:
        *msg_type = GENAVB_MSG_MEDIA_STACK_DISCONNECT;
        media->media_stack_disconnect.stream_index = 4;
        media->media_stack_disconnect.port = 0;
        media->media_stack_disconnect.stream_class = SR_CLASS_B;
        media->media_stack_disconnect.direction = AVTP_DIRECTION_TALKER;
        media->media_stack_disconnect.stream_id[0] = 0x10;
        media->media_stack_disconnect.stream_id[7] = 0x04;
        break;
    case 3:
        *msg_type = GENAVB_MSG_MEDIA_STACK_BIND;
        break;
    default:
        return -1;
    }
    *msg_len = sizeof(*media);
    return GENAVB_SUCCESS;
}

/* Unused stream/clock functions are provided because the shim object contains
 * both the talker and AVDECC adapters. */
int genavb_stream_create(struct genavb_handle *genavb, struct genavb_stream_handle **stream,
                         const struct genavb_stream_params *params, unsigned int *batch_size,
                         genavb_stream_create_flags_t flags)
{
    (void)genavb; (void)stream; (void)params; (void)batch_size; (void)flags;
    return -1;
}
int genavb_stream_destroy(struct genavb_stream_handle *stream) { (void)stream; return 0; }
unsigned int genavb_stream_presentation_offset(const struct genavb_stream_handle *stream)
{ (void)stream; return 0; }
genavb_clock_id_t genavb_stream_avtp_clock(const struct genavb_stream_handle *stream)
{ (void)stream; return GENAVB_CLOCK_AVTP_0; }
int genavb_clock_gettime64(genavb_clock_id_t id, uint64_t *ns)
{ (void)id; *ns = 0; return GENAVB_SUCCESS; }
int genavb_stream_send(const struct genavb_stream_handle *stream, const void *data,
                       unsigned int data_len, const struct genavb_event *event,
                       unsigned int event_len)
{ (void)stream; (void)data; (void)data_len; (void)event; (void)event_len; return -1; }

int main(void)
{
    struct aurora_genavb_avdecc_event event;
    void *control = aurora_genavb_avdecc_create();

    if (!control || aurora_genavb_avdecc_open(control) != 0)
        return 10;
    if (init_calls != 1 || control_open_calls != 1 || aurora_genavb_avdecc_rx_fd(control) != 42)
        return 11;

    receive_case = 0;
    if (aurora_genavb_avdecc_receive(control, &event) != 1)
        return 12;
    if (event.kind != AURORA_GENAVB_AVDECC_EVENT_CONNECT || event.stream_index != 4 ||
        event.direction != AVTP_DIRECTION_TALKER || event.stream_class != SR_CLASS_B ||
        event.sample_rate_hz != 48000 || event.channels != 2 || event.bit_depth != 24 ||
        event.stream_id[7] != 0x04 || event.destination_mac[5] != 0x04)
        return 13;

    receive_case = 1;
    if (aurora_genavb_avdecc_receive(control, &event) != -2)
        return 14;

    receive_case = 2;
    if (aurora_genavb_avdecc_receive(control, &event) != 1)
        return 15;
    if (event.kind != AURORA_GENAVB_AVDECC_EVENT_DISCONNECT || event.stream_index != 4 ||
        event.direction != AVTP_DIRECTION_TALKER || event.stream_id[7] != 0x04)
        return 16;

    receive_case = 3;
    if (aurora_genavb_avdecc_receive(control, &event) != 0)
        return 17;

    if (aurora_genavb_avdecc_close(control) != 0)
        return 18;
    aurora_genavb_avdecc_destroy(control);
    if (control_close_calls != 1 || exit_calls != 1)
        return 19;

    printf("aurora-genavb-avdecc: PASS channel=media-stack connect=aaf24-48k-stereo invalid-format=fail-closed disconnect=propagated bind=ignored runtime-init=1\n");
    return 0;
}
