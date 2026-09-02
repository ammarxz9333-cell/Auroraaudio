#include "aurora_transport.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define TX_CAPACITY (64u * 1024u)
#define FRAME_CAPACITY (AURORA_USB_HEADER_LEN + AURORA_STM32_PCM_PERIOD_BYTES)

struct mock_ctx {
    uint8_t tx[TX_CAPACITY];
    size_t tx_len;
    int muted;
    int queue_should_fail;
    unsigned queue_calls;
    size_t last_queue_len;
    uint64_t last_queue_pts;
    uint32_t last_queue_flags;
    uint64_t sink_counter;
    uint64_t source_counter;
    uint32_t queued_frames;
};

static int mock_usb_send(void *opaque, const uint8_t *data, size_t len)
{
    struct mock_ctx *ctx = opaque;
    if (!ctx || !data || ctx->tx_len + len > sizeof(ctx->tx))
        return -1;
    memcpy(ctx->tx + ctx->tx_len, data, len);
    ctx->tx_len += len;
    return 0;
}

static int mock_queue_pcm(void *opaque, const uint8_t *pcm, size_t len,
                          uint64_t pts_48k, uint32_t flags)
{
    struct mock_ctx *ctx = opaque;
    if (!ctx || !pcm)
        return -1;
    ctx->queue_calls++;
    ctx->last_queue_len = len;
    ctx->last_queue_pts = pts_48k;
    ctx->last_queue_flags = flags;
    if (ctx->queue_should_fail)
        return -1;
    ctx->queued_frames += AURORA_USB_PERIOD_FRAMES;
    return 0;
}

static void mock_set_mute(void *opaque, int muted)
{
    struct mock_ctx *ctx = opaque;
    assert(ctx != NULL);
    ctx->muted = muted ? 1 : 0;
}

static uint64_t mock_sink_counter(void *opaque)
{
    return ((struct mock_ctx *)opaque)->sink_counter;
}

static uint64_t mock_source_counter(void *opaque)
{
    return ((struct mock_ctx *)opaque)->source_counter;
}

static uint32_t mock_queued_frames(void *opaque)
{
    return ((struct mock_ctx *)opaque)->queued_frames;
}

static void tx_reset(struct mock_ctx *ctx)
{
    ctx->tx_len = 0;
    memset(ctx->tx, 0, sizeof(ctx->tx));
}

static unsigned tx_count_kind(const struct mock_ctx *ctx, uint16_t wanted)
{
    size_t off = 0;
    unsigned count = 0;

    while (off + AURORA_USB_HEADER_LEN <= ctx->tx_len) {
        const uint8_t *h = ctx->tx + off;
        assert(aurora_usb_read_le32(h + 0) == AURORA_USB_MAGIC_U32);
        assert(aurora_usb_read_le16(h + 4) == AURORA_USB_VERSION);
        uint16_t kind = aurora_usb_read_le16(h + 6);
        uint32_t payload_len = aurora_usb_read_le32(h + 24);
        size_t frame_len = AURORA_USB_HEADER_LEN + (size_t)payload_len;
        assert(off + frame_len <= ctx->tx_len);
        if (kind == wanted)
            count++;
        off += frame_len;
    }
    assert(off == ctx->tx_len);
    return count;
}

static uint32_t tx_first_flags_for_kind(const struct mock_ctx *ctx, uint16_t wanted)
{
    size_t off = 0;
    while (off + AURORA_USB_HEADER_LEN <= ctx->tx_len) {
        const uint8_t *h = ctx->tx + off;
        uint16_t kind = aurora_usb_read_le16(h + 6);
        uint32_t payload_len = aurora_usb_read_le32(h + 24);
        if (kind == wanted)
            return aurora_usb_read_le32(h + 8);
        off += AURORA_USB_HEADER_LEN + (size_t)payload_len;
    }
    return 0;
}

static size_t make_frame(uint8_t *out, size_t out_cap, uint16_t kind,
                         uint32_t flags, uint32_t sequence, uint64_t pts_48k,
                         uint32_t aux, const uint8_t *payload, uint32_t payload_len)
{
    size_t total = AURORA_USB_HEADER_LEN + (size_t)payload_len;
    assert(out != NULL);
    assert(total <= out_cap);
    assert(payload_len == 0 || payload != NULL);

    memset(out, 0, total);
    aurora_usb_write_le32(out + 0, AURORA_USB_MAGIC_U32);
    aurora_usb_write_le16(out + 4, AURORA_USB_VERSION);
    aurora_usb_write_le16(out + 6, kind);
    aurora_usb_write_le32(out + 8, flags);
    aurora_usb_write_le32(out + 12, sequence);
    aurora_usb_write_le64(out + 16, pts_48k);
    aurora_usb_write_le32(out + 24, payload_len);
    aurora_usb_write_le32(out + 28, aux);
    if (payload_len)
        memcpy(out + AURORA_USB_HEADER_LEN, payload, payload_len);
    return total;
}

static void make_config_payload(uint8_t payload[sizeof(struct aurora_usb_config_v1)],
                                const uint8_t layout_hash[32])
{
    memset(payload, 0, sizeof(struct aurora_usb_config_v1));
    aurora_usb_write_le32(payload + 0, AURORA_USB_SAMPLE_RATE_HZ);
    aurora_usb_write_le16(payload + 4, AURORA_USB_PERIOD_FRAMES);
    aurora_usb_write_le16(payload + 6, AURORA_USB_CHANNELS_7_1_4);
    aurora_usb_write_le16(payload + 8, AURORA_USB_PCM_FORMAT_S32LE);
    aurora_usb_write_le16(payload + 10, AURORA_USB_LAYOUT_ID_7_1_4);
    aurora_usb_write_le32(payload + 12, 0);
    memcpy(payload + 16, layout_hash, 32);
}

static void feed_split(struct aurora_transport *t, const uint8_t *frame, size_t len)
{
    size_t a = len > 7 ? 7 : len;
    size_t b = len > a + 31 ? 31 : len - a;
    assert(aurora_transport_receive(t, frame, a) == 0);
    assert(aurora_transport_receive(t, frame + a, b) == 0);
    assert(aurora_transport_receive(t, frame + a + b, len - a - b) == 0);
}

int main(void)
{
    static uint8_t frame[FRAME_CAPACITY];
    static uint8_t pcm[AURORA_STM32_PCM_PERIOD_BYTES];
    uint8_t config_payload[sizeof(struct aurora_usb_config_v1)];
    uint8_t good_hash[32];
    uint8_t bad_hash[32];
    struct mock_ctx ctx;
    struct aurora_transport transport;
    struct aurora_transport_io io;

    memset(&ctx, 0, sizeof(ctx));
    memset(&io, 0, sizeof(io));
    memset(good_hash, 0x5a, sizeof(good_hash));
    memset(bad_hash, 0xa5, sizeof(bad_hash));
    memset(pcm, 0, sizeof(pcm));

    io.ctx = &ctx;
    io.usb_send = mock_usb_send;
    io.queue_pcm_period = mock_queue_pcm;
    io.set_amplifier_mute = mock_set_mute;
    io.sink_sample_counter = mock_sink_counter;
    io.source_sample_counter = mock_source_counter;
    io.queued_playback_frames = mock_queued_frames;

    aurora_transport_init(&transport, &io, good_hash);
    assert(aurora_transport_get_state(&transport) == AURORA_TRANSPORT_WAIT_CONFIG);
    assert(ctx.muted == 1);
    assert(aurora_transport_get_xrun_count(&transport) == 0);

    /* PCM before CONFIG must never unmute or reach the realtime queue. */
    tx_reset(&ctx);
    size_t frame_len = make_frame(
        frame, sizeof(frame), AURORA_USB_PCM_S32LE, AURORA_USB_FLAG_PTS_VALID,
        0, 1000, AURORA_USB_PCM_AUX(AURORA_USB_CHANNELS_7_1_4,
                                    AURORA_USB_PERIOD_FRAMES),
        pcm, sizeof(pcm));
    feed_split(&transport, frame, frame_len);
    assert(ctx.queue_calls == 0);
    assert(ctx.muted == 1);
    assert(aurora_transport_get_state(&transport) == AURORA_TRANSPORT_WAIT_CONFIG);
    assert(tx_count_kind(&ctx, AURORA_USB_ERROR) == 1);

    /* A bad layout hash is rejected fail-closed. */
    tx_reset(&ctx);
    make_config_payload(config_payload, bad_hash);
    frame_len = make_frame(frame, sizeof(frame), AURORA_USB_CONFIG, 0, 1, 0, 0,
                           config_payload, sizeof(config_payload));
    feed_split(&transport, frame, frame_len);
    assert(ctx.muted == 1);
    assert(aurora_transport_get_state(&transport) == AURORA_TRANSPORT_WAIT_CONFIG);
    assert(tx_count_kind(&ctx, AURORA_USB_ERROR) == 1);

    /* Correct CONFIG arms the transport but deliberately remains muted. */
    tx_reset(&ctx);
    make_config_payload(config_payload, good_hash);
    frame_len = make_frame(frame, sizeof(frame), AURORA_USB_CONFIG, 0, 2, 0, 0,
                           config_payload, sizeof(config_payload));
    feed_split(&transport, frame, frame_len);
    assert(ctx.muted == 1);
    assert(aurora_transport_get_state(&transport) == AURORA_TRANSPORT_ARMED_MUTED);
    assert(tx_count_kind(&ctx, AURORA_USB_ACK) == 1);

    /* First valid PCM period is the only transition that unmutes playback. */
    tx_reset(&ctx);
    frame_len = make_frame(
        frame, sizeof(frame), AURORA_USB_PCM_S32LE, AURORA_USB_FLAG_PTS_VALID,
        3, 4096, AURORA_USB_PCM_AUX(AURORA_USB_CHANNELS_7_1_4,
                                    AURORA_USB_PERIOD_FRAMES),
        pcm, sizeof(pcm));
    feed_split(&transport, frame, frame_len);
    assert(ctx.queue_calls == 1);
    assert(ctx.last_queue_len == sizeof(pcm));
    assert(ctx.last_queue_pts == 4096);
    assert(ctx.last_queue_flags == AURORA_USB_FLAG_PTS_VALID);
    assert(ctx.muted == 0);
    assert(aurora_transport_get_state(&transport) == AURORA_TRANSPORT_STREAMING);

    /* A queue failure is an xrun: immediately mute and report recovery state. */
    tx_reset(&ctx);
    ctx.queue_should_fail = 1;
    frame_len = make_frame(
        frame, sizeof(frame), AURORA_USB_PCM_S32LE, AURORA_USB_FLAG_PTS_VALID,
        4, 4352, AURORA_USB_PCM_AUX(AURORA_USB_CHANNELS_7_1_4,
                                    AURORA_USB_PERIOD_FRAMES),
        pcm, sizeof(pcm));
    feed_split(&transport, frame, frame_len);
    assert(ctx.queue_calls == 2);
    assert(ctx.muted == 1);
    assert(aurora_transport_get_state(&transport) == AURORA_TRANSPORT_XRUN_MUTED);
    assert(aurora_transport_get_xrun_count(&transport) == 1);
    assert(tx_count_kind(&ctx, AURORA_USB_CLOCK_REPORT) == 1);
    assert(tx_count_kind(&ctx, AURORA_USB_ERROR) == 1);
    assert((tx_first_flags_for_kind(&ctx, AURORA_USB_CLOCK_REPORT) &
            AURORA_USB_FLAG_XRUN_RECOVERY) != 0);
    ctx.queue_should_fail = 0;

    /* USB reset throws away CONFIG state and always returns to mute. */
    tx_reset(&ctx);
    aurora_transport_usb_reset(&transport);
    assert(ctx.muted == 1);
    assert(aurora_transport_get_state(&transport) == AURORA_TRANSPORT_WAIT_CONFIG);
    frame_len = make_frame(
        frame, sizeof(frame), AURORA_USB_PCM_S32LE, AURORA_USB_FLAG_PTS_VALID,
        0, 8192, AURORA_USB_PCM_AUX(AURORA_USB_CHANNELS_7_1_4,
                                    AURORA_USB_PERIOD_FRAMES),
        pcm, sizeof(pcm));
    feed_split(&transport, frame, frame_len);
    assert(ctx.muted == 1);
    assert(ctx.queue_calls == 2);
    assert(tx_count_kind(&ctx, AURORA_USB_ERROR) == 1);

    /* Capture and clock-report transmit paths use the same v1 framing. */
    tx_reset(&ctx);
    ctx.sink_counter = 9000;
    ctx.source_counter = 8800;
    ctx.queued_frames = 512;
    {
        const uint8_t iec[] = {0x72, 0xf8, 0x1f, 0x4e, 0x15, 0x00};
        assert(aurora_transport_send_iec61937(&transport, iec, sizeof(iec),
                                              8800, 0) == 0);
        assert(aurora_transport_send_clock_report(&transport, 0) == 0);
    }
    assert(tx_count_kind(&ctx, AURORA_USB_ENCODED_IEC61937) == 1);
    assert(tx_count_kind(&ctx, AURORA_USB_CLOCK_REPORT) == 1);

    puts("STM32 Aurora transport state-machine tests passed");
    return 0;
}
