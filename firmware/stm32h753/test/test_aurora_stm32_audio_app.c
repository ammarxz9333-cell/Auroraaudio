#include "aurora_stm32_audio_app.h"

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
    unsigned queue_calls;
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
    if (!ctx || !pcm || len != AURORA_STM32_PCM_PERIOD_BYTES)
        return -1;
    ctx->queue_calls++;
    ctx->last_queue_pts = pts_48k;
    ctx->last_queue_flags = flags;
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

static void clear_tx(struct mock_ctx *ctx)
{
    ctx->tx_len = 0;
    memset(ctx->tx, 0, sizeof(ctx->tx));
}

static size_t make_frame(uint8_t *out, size_t out_cap, uint16_t kind,
                         uint32_t flags, uint32_t sequence, uint64_t pts_48k,
                         uint32_t aux, const uint8_t *payload, uint32_t payload_len)
{
    size_t total = AURORA_USB_HEADER_LEN + (size_t)payload_len;
    assert(total <= out_cap);
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
    memcpy(payload + 16, layout_hash, 32);
}

int main(void)
{
    struct mock_ctx ctx;
    struct aurora_transport_io io;
    struct aurora_stm32_audio_app app;
    uint8_t layout_hash[32];
    uint8_t scratch[128];
    static uint8_t frame[FRAME_CAPACITY];
    static uint8_t pcm[AURORA_STM32_PCM_PERIOD_BYTES];
    uint8_t config_payload[sizeof(struct aurora_usb_config_v1)];

    memset(&ctx, 0, sizeof(ctx));
    memset(&io, 0, sizeof(io));
    memset(layout_hash, 0x5a, sizeof(layout_hash));
    memset(pcm, 0, sizeof(pcm));

    io.ctx = &ctx;
    io.usb_send = mock_usb_send;
    io.queue_pcm_period = mock_queue_pcm;
    io.set_amplifier_mute = mock_set_mute;
    io.sink_sample_counter = mock_sink_counter;
    io.source_sample_counter = mock_source_counter;
    io.queued_playback_frames = mock_queued_frames;

    aurora_stm32_audio_app_init(&app, &io, layout_hash);
    assert(ctx.muted == 1);
    assert(aurora_transport_get_state(&app.transport) == AURORA_TRANSPORT_WAIT_CONFIG);
    assert(app.capture.have_carrier_rate == 0);

    const uint32_t slots[] = {
        0xf8721111u, 0x4e1f2222u,
        0x00153333u, 0x00064444u,
        0x0b775555u, 0x12346666u,
        0x56787777u, 0x00008888u,
    };

    /* The integration core owns capture continuity: first rate is clean. */
    clear_tx(&ctx);
    assert(aurora_stm32_audio_app_earc_dma_s32_high_words(
               &app, slots, 8, 400, 192000, 0,
               scratch, sizeof(scratch)) == 0);
    assert(aurora_usb_read_le16(ctx.tx + 6) == AURORA_USB_ENCODED_IEC61937);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_DISCONTINUITY) == 0);

    /* A physical carrier-rate transition is marked exactly once by capture. */
    clear_tx(&ctx);
    assert(aurora_stm32_audio_app_earc_dma_s32_high_words(
               &app, slots, 8, 400, 96000, 0,
               scratch, sizeof(scratch)) == 0);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_DISCONTINUITY) != 0);

    /* USB session reset also clears capture continuity ownership. */
    aurora_stm32_audio_app_usb_reset(&app);
    assert(ctx.muted == 1);
    assert(app.capture.have_carrier_rate == 0);
    clear_tx(&ctx);
    assert(aurora_stm32_audio_app_earc_dma_s32_high_words(
               &app, slots, 8, 400, 96000, 0,
               scratch, sizeof(scratch)) == 0);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_DISCONTINUITY) == 0);

    /* S6 -> STM32 CONFIG and PCM use the same integrated transport instance. */
    clear_tx(&ctx);
    make_config_payload(config_payload, layout_hash);
    size_t frame_len = make_frame(
        frame, sizeof(frame), AURORA_USB_CONFIG, 0, 1, 0, 0,
        config_payload, sizeof(config_payload));
    assert(aurora_stm32_audio_app_usb_receive(&app, frame, frame_len) == 0);
    assert(aurora_transport_get_state(&app.transport) == AURORA_TRANSPORT_ARMED_MUTED);
    assert(ctx.muted == 1);
    assert(aurora_usb_read_le16(ctx.tx + 6) == AURORA_USB_ACK);

    clear_tx(&ctx);
    frame_len = make_frame(
        frame, sizeof(frame), AURORA_USB_PCM_S32LE, AURORA_USB_FLAG_PTS_VALID,
        2, 4096,
        AURORA_USB_PCM_AUX(AURORA_USB_CHANNELS_7_1_4,
                           AURORA_USB_PERIOD_FRAMES),
        pcm, sizeof(pcm));
    assert(aurora_stm32_audio_app_usb_receive(&app, frame, frame_len) == 0);
    assert(ctx.queue_calls == 1);
    assert(ctx.last_queue_pts == 4096);
    assert(ctx.last_queue_flags == AURORA_USB_FLAG_PTS_VALID);
    assert(ctx.muted == 0);
    assert(aurora_transport_get_state(&app.transport) == AURORA_TRANSPORT_STREAMING);

    /* Clock and XRUN events terminate at the same transport owner. */
    ctx.sink_counter = 48000;
    ctx.source_counter = 48005;
    ctx.queued_frames = 120;
    clear_tx(&ctx);
    assert(aurora_stm32_audio_app_send_clock_report(&app, 0) == 0);
    assert(aurora_usb_read_le16(ctx.tx + 6) == AURORA_USB_CLOCK_REPORT);
    assert(aurora_usb_read_le64(ctx.tx + AURORA_USB_HEADER_LEN + 0) == 48000);
    assert(aurora_usb_read_le64(ctx.tx + AURORA_USB_HEADER_LEN + 8) == 48005);
    assert(aurora_usb_read_le32(ctx.tx + AURORA_USB_HEADER_LEN + 16) == 120);

    clear_tx(&ctx);
    aurora_stm32_audio_app_playback_xrun(&app);
    assert(ctx.muted == 1);
    assert(aurora_transport_get_state(&app.transport) == AURORA_TRANSPORT_XRUN_MUTED);
    assert(aurora_transport_get_xrun_count(&app.transport) == 1);
    assert(aurora_usb_read_le16(ctx.tx + 6) == AURORA_USB_CLOCK_REPORT);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_XRUN_RECOVERY) != 0);

    puts("STM32 integrated Aurora audio app tests passed");
    return 0;
}
