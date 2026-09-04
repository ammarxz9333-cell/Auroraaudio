#include "aurora_transport.h"

#include <string.h>

enum aurora_error_code_v1 {
    AURORA_ERR_FRAMING = 1,
    AURORA_ERR_UNSUPPORTED_KIND = 2,
    AURORA_ERR_CONFIG = 3,
    AURORA_ERR_NOT_CONFIGURED = 4,
    AURORA_ERR_PCM_SHAPE = 5,
    AURORA_ERR_PCM_QUEUE = 6,
};

static void set_mute(struct aurora_transport *t, int muted)
{
    if (t->io.set_amplifier_mute)
        t->io.set_amplifier_mute(t->io.ctx, muted);
}

static int send_frame(struct aurora_transport *t, uint16_t kind,
                      uint32_t flags, uint64_t pts_48k, uint32_t aux,
                      const uint8_t *payload, uint32_t payload_len)
{
    uint8_t header[AURORA_USB_HEADER_LEN] = {0};
    if (!t || !t->io.usb_send)
        return -1;
    if (payload_len > AURORA_USB_MAX_FRAME - AURORA_USB_HEADER_LEN)
        return -1;
    if (payload_len != 0 && !payload)
        return -1;

    aurora_usb_write_le32(header + 0, AURORA_USB_MAGIC_U32);
    aurora_usb_write_le16(header + 4, AURORA_USB_VERSION);
    aurora_usb_write_le16(header + 6, kind);
    aurora_usb_write_le32(header + 8, flags);
    aurora_usb_write_le32(header + 12, t->tx_sequence++);
    aurora_usb_write_le64(header + 16, pts_48k);
    aurora_usb_write_le32(header + 24, payload_len);
    aurora_usb_write_le32(header + 28, aux);

    if (t->io.usb_send(t->io.ctx, header, sizeof(header)) != 0)
        return -1;
    if (payload_len && t->io.usb_send(t->io.ctx, payload, payload_len) != 0)
        return -1;
    return 0;
}

static int send_ack(struct aurora_transport *t, uint16_t ack_kind)
{
    uint8_t payload[4] = {0};
    aurora_usb_write_le16(payload + 0, ack_kind);
    aurora_usb_write_le16(payload + 2, 0);
    return send_frame(t, AURORA_USB_ACK, 0, 0, 0, payload, sizeof(payload));
}

static int send_error(struct aurora_transport *t, uint16_t offending_kind,
                      uint16_t error_code)
{
    uint8_t payload[4] = {0};
    aurora_usb_write_le16(payload + 0, offending_kind);
    aurora_usb_write_le16(payload + 2, error_code);
    return send_frame(t, AURORA_USB_ERROR, 0, 0, 0, payload, sizeof(payload));
}

static int config_is_valid(const struct aurora_transport *t,
                           const uint8_t *payload, size_t len)
{
    if (len != sizeof(struct aurora_usb_config_v1))
        return 0;
    if (aurora_usb_read_le32(payload + 0) != AURORA_USB_SAMPLE_RATE_HZ)
        return 0;
    if (aurora_usb_read_le16(payload + 4) != AURORA_USB_PERIOD_FRAMES)
        return 0;
    if (aurora_usb_read_le16(payload + 6) != AURORA_USB_CHANNELS_7_1_4)
        return 0;
    if (aurora_usb_read_le16(payload + 8) != AURORA_USB_PCM_FORMAT_S32LE)
        return 0;
    if (aurora_usb_read_le16(payload + 10) != AURORA_USB_LAYOUT_ID_7_1_4)
        return 0;
    if (aurora_usb_read_le32(payload + 12) != 0)
        return 0;
    return memcmp(payload + 16, t->expected_layout_hash, 32) == 0;
}

static int handle_config(struct aurora_transport *t, const uint8_t *payload,
                         size_t len)
{
    set_mute(t, 1);
    t->configured = 0;
    t->state = AURORA_TRANSPORT_WAIT_CONFIG;
    if (!config_is_valid(t, payload, len)) {
        (void)send_error(t, AURORA_USB_CONFIG, AURORA_ERR_CONFIG);
        return 0;
    }
    t->configured = 1;
    t->state = AURORA_TRANSPORT_ARMED_MUTED;
    return send_ack(t, AURORA_USB_CONFIG);
}

static int handle_pcm(struct aurora_transport *t, const uint8_t *frame,
                      size_t frame_len)
{
    const uint8_t *payload = frame + AURORA_USB_HEADER_LEN;
    uint32_t payload_len = aurora_usb_read_le32(frame + 24);
    uint32_t aux = aurora_usb_read_le32(frame + 28);
    uint32_t flags = aurora_usb_read_le32(frame + 8);
    uint64_t pts_48k = aurora_usb_read_le64(frame + 16);

    if (!t->configured) {
        set_mute(t, 1);
        t->state = AURORA_TRANSPORT_WAIT_CONFIG;
        (void)send_error(t, AURORA_USB_PCM_S32LE, AURORA_ERR_NOT_CONFIGURED);
        return 0;
    }
    if (frame_len != AURORA_USB_HEADER_LEN + (size_t)payload_len ||
        payload_len != AURORA_REALTIME_MCU_PCM_PERIOD_BYTES ||
        AURORA_USB_PCM_AUX_CHANNELS(aux) != AURORA_USB_CHANNELS_7_1_4 ||
        AURORA_USB_PCM_AUX_FRAMES(aux) != AURORA_USB_PERIOD_FRAMES) {
        set_mute(t, 1);
        t->state = AURORA_TRANSPORT_ARMED_MUTED;
        (void)send_error(t, AURORA_USB_PCM_S32LE, AURORA_ERR_PCM_SHAPE);
        return 0;
    }
    if (flags & AURORA_USB_FLAG_DISCONTINUITY) {
        set_mute(t, 1);
        t->state = AURORA_TRANSPORT_ARMED_MUTED;
    }
    if (!t->io.queue_pcm_period ||
        t->io.queue_pcm_period(t->io.ctx, payload, payload_len,
                               pts_48k, flags) != 0) {
        aurora_transport_report_xrun(t);
        (void)send_error(t, AURORA_USB_PCM_S32LE, AURORA_ERR_PCM_QUEUE);
        return 0;
    }
    t->state = AURORA_TRANSPORT_STREAMING;
    set_mute(t, 0);
    return 0;
}

static int handle_frame(void *opaque, const uint8_t *frame, size_t frame_len)
{
    struct aurora_transport *t = opaque;
    uint16_t kind = aurora_usb_read_le16(frame + 6);
    uint32_t payload_len = aurora_usb_read_le32(frame + 24);
    const uint8_t *payload = frame + AURORA_USB_HEADER_LEN;

    if (frame_len != AURORA_USB_HEADER_LEN + (size_t)payload_len)
        return -1;
    switch (kind) {
    case AURORA_USB_CONFIG:
        return handle_config(t, payload, payload_len);
    case AURORA_USB_PCM_S32LE:
        return handle_pcm(t, frame, frame_len);
    case AURORA_USB_PING:
        return send_frame(t, AURORA_USB_PONG, 0, 0, 0, NULL, 0);
    case AURORA_USB_PONG:
    case AURORA_USB_ACK:
    case AURORA_USB_ERROR:
        return 0;
    default:
        (void)send_error(t, kind, AURORA_ERR_UNSUPPORTED_KIND);
        return 0;
    }
}

void aurora_transport_init(struct aurora_transport *t,
                           const struct aurora_transport_io *io,
                           const uint8_t expected_layout_hash[32])
{
    if (!t)
        return;
    memset(t, 0, sizeof(*t));
    if (io)
        t->io = *io;
    if (expected_layout_hash)
        memcpy(t->expected_layout_hash, expected_layout_hash, 32);
    aurora_usb_stream_v1_init(&t->rx_stream, t->rx_storage,
                              sizeof(t->rx_storage));
    t->state = AURORA_TRANSPORT_WAIT_CONFIG;
    set_mute(t, 1);
}

void aurora_transport_usb_reset(struct aurora_transport *t)
{
    if (!t)
        return;
    aurora_usb_stream_v1_reset(&t->rx_stream);
    t->tx_sequence = 0;
    t->configured = 0;
    t->state = AURORA_TRANSPORT_WAIT_CONFIG;
    set_mute(t, 1);
}

int aurora_transport_receive(struct aurora_transport *t,
                             const uint8_t *data, size_t len)
{
    int rc;
    if (!t)
        return -1;
    rc = aurora_usb_stream_v1_feed(&t->rx_stream, data, len, handle_frame, t);
    if (rc != AURORA_USB_STREAM_OK) {
        aurora_usb_stream_v1_reset(&t->rx_stream);
        t->configured = 0;
        t->state = AURORA_TRANSPORT_WAIT_CONFIG;
        set_mute(t, 1);
        (void)send_error(t, 0, AURORA_ERR_FRAMING);
        return -1;
    }
    return 0;
}

int aurora_transport_send_iec61937(struct aurora_transport *t,
                                   const uint8_t *payload, size_t len,
                                   uint64_t pts_48k, uint32_t flags)
{
    if (!t || !payload || len == 0 ||
        len > AURORA_USB_MAX_FRAME - AURORA_USB_HEADER_LEN)
        return -1;
    return send_frame(t, AURORA_USB_ENCODED_IEC61937,
                      flags | AURORA_USB_FLAG_PTS_VALID,
                      pts_48k, 0, payload, (uint32_t)len);
}

int aurora_transport_send_clock_report(struct aurora_transport *t,
                                       uint32_t extra_flags)
{
    uint8_t payload[sizeof(struct aurora_usb_clock_report_v1)] = {0};
    uint64_t sink = 0;
    uint64_t source = 0;
    uint32_t queued = 0;
    if (!t)
        return -1;
    if (t->io.sink_sample_counter)
        sink = t->io.sink_sample_counter(t->io.ctx);
    if (t->io.source_sample_counter)
        source = t->io.source_sample_counter(t->io.ctx);
    if (t->io.queued_playback_frames)
        queued = t->io.queued_playback_frames(t->io.ctx);
    aurora_usb_write_le64(payload + 0, sink);
    aurora_usb_write_le64(payload + 8, source);
    aurora_usb_write_le32(payload + 16, queued);
    aurora_usb_write_le32(payload + 20, extra_flags);
    return send_frame(t, AURORA_USB_CLOCK_REPORT,
                      extra_flags & AURORA_USB_FLAG_XRUN_RECOVERY,
                      sink, 0, payload, sizeof(payload));
}

void aurora_transport_report_xrun(struct aurora_transport *t)
{
    if (!t)
        return;
    t->xrun_count++;
    t->state = AURORA_TRANSPORT_XRUN_MUTED;
    set_mute(t, 1);
    (void)aurora_transport_send_clock_report(t, AURORA_USB_FLAG_XRUN_RECOVERY);
}

enum aurora_transport_state aurora_transport_get_state(
    const struct aurora_transport *t)
{
    return t ? t->state : AURORA_TRANSPORT_WAIT_CONFIG;
}

uint32_t aurora_transport_get_xrun_count(const struct aurora_transport *t)
{
    return t ? t->xrun_count : 0;
}
