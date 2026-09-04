#include "aurora_realtime_mcu_app.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define TX_CAPACITY (64u * 1024u)
#define FRAME_CAPACITY (AURORA_USB_HEADER_LEN + AURORA_REALTIME_MCU_PCM_PERIOD_BYTES)

struct mock_ctx {
    uint8_t tx[TX_CAPACITY];
    size_t tx_len;
    int muted;
    unsigned queue_calls;
    uint64_t sink_counter;
    uint64_t source_counter;
    uint32_t queued_frames;
};

static int mock_usb_send(void *opaque, const uint8_t *data, size_t len)
{
    struct mock_ctx *ctx = opaque;
    if (!ctx || !data || ctx->tx_len + len > sizeof(ctx->tx)) return -1;
    memcpy(ctx->tx + ctx->tx_len, data, len); ctx->tx_len += len; return 0;
}
static int mock_queue_pcm(void *opaque, const uint8_t *pcm, size_t len,
                          uint64_t pts, uint32_t flags)
{
    struct mock_ctx *ctx = opaque;
    (void)pts; (void)flags;
    if (!ctx || !pcm || len != AURORA_REALTIME_MCU_PCM_PERIOD_BYTES) return -1;
    ctx->queue_calls++; ctx->queued_frames += AURORA_USB_PERIOD_FRAMES; return 0;
}
static void mock_mute(void *opaque, int muted) { ((struct mock_ctx *)opaque)->muted = muted ? 1 : 0; }
static uint64_t mock_sink(void *opaque) { return ((struct mock_ctx *)opaque)->sink_counter; }
static uint64_t mock_source(void *opaque) { return ((struct mock_ctx *)opaque)->source_counter; }
static uint32_t mock_queued(void *opaque) { return ((struct mock_ctx *)opaque)->queued_frames; }
static void clear_tx(struct mock_ctx *ctx) { ctx->tx_len = 0; memset(ctx->tx, 0, sizeof(ctx->tx)); }

static size_t make_frame(uint8_t *out, uint16_t kind, uint32_t flags,
                         uint32_t seq, uint64_t pts, uint32_t aux,
                         const uint8_t *payload, uint32_t payload_len)
{
    size_t total = AURORA_USB_HEADER_LEN + (size_t)payload_len;
    memset(out, 0, total);
    aurora_usb_write_le32(out + 0, AURORA_USB_MAGIC_U32);
    aurora_usb_write_le16(out + 4, AURORA_USB_VERSION);
    aurora_usb_write_le16(out + 6, kind);
    aurora_usb_write_le32(out + 8, flags);
    aurora_usb_write_le32(out + 12, seq);
    aurora_usb_write_le64(out + 16, pts);
    aurora_usb_write_le32(out + 24, payload_len);
    aurora_usb_write_le32(out + 28, aux);
    if (payload_len) memcpy(out + AURORA_USB_HEADER_LEN, payload, payload_len);
    return total;
}

int main(void)
{
    struct mock_ctx ctx = {0};
    struct aurora_transport_io io = {0};
    struct aurora_realtime_mcu_app app;
    uint8_t hash[32];
    uint8_t scratch[128];
    uint8_t config[sizeof(struct aurora_usb_config_v1)] = {0};
    static uint8_t frame[FRAME_CAPACITY];
    static uint8_t pcm[AURORA_REALTIME_MCU_PCM_PERIOD_BYTES];
    const uint32_t slots[] = {
        0xf8721111u,0x4e1f2222u,0x00153333u,0x00064444u,
        0x0b775555u,0x12346666u,0x56787777u,0x00008888u,
    };
    memset(hash, 0x5a, sizeof(hash));
    io.ctx=&ctx; io.usb_send=mock_usb_send; io.queue_pcm_period=mock_queue_pcm;
    io.set_amplifier_mute=mock_mute; io.sink_sample_counter=mock_sink;
    io.source_sample_counter=mock_source; io.queued_playback_frames=mock_queued;

    aurora_realtime_mcu_app_init(&app, &io, hash);
    assert(ctx.muted == 1);
    assert(app.capture.have_carrier_rate == 0);

    assert(aurora_realtime_mcu_app_earc_dma_s32_high_words(
        &app, slots, 8, 400, 192000, 0, scratch, sizeof(scratch)) == 0);
    assert(aurora_usb_read_le16(ctx.tx + 6) == AURORA_USB_ENCODED_IEC61937);

    clear_tx(&ctx);
    assert(aurora_realtime_mcu_app_earc_dma_s32_high_words(
        &app, slots, 8, 400, 96000, 0, scratch, sizeof(scratch)) == 0);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_DISCONTINUITY) != 0);

    aurora_realtime_mcu_app_usb_reset(&app);
    assert(ctx.muted == 1 && app.capture.have_carrier_rate == 0);

    aurora_usb_write_le32(config + 0, AURORA_USB_SAMPLE_RATE_HZ);
    aurora_usb_write_le16(config + 4, AURORA_USB_PERIOD_FRAMES);
    aurora_usb_write_le16(config + 6, AURORA_USB_CHANNELS_7_1_4);
    aurora_usb_write_le16(config + 8, AURORA_USB_PCM_FORMAT_S32LE);
    aurora_usb_write_le16(config + 10, AURORA_USB_LAYOUT_ID_7_1_4);
    memcpy(config + 16, hash, 32);
    clear_tx(&ctx);
    size_t len = make_frame(frame, AURORA_USB_CONFIG, 0, 1, 0, 0, config, sizeof(config));
    assert(aurora_realtime_mcu_app_usb_receive(&app, frame, len) == 0);
    assert(aurora_transport_get_state(&app.transport) == AURORA_TRANSPORT_ARMED_MUTED);

    clear_tx(&ctx);
    len = make_frame(frame, AURORA_USB_PCM_S32LE, AURORA_USB_FLAG_PTS_VALID, 2, 4096,
                     AURORA_USB_PCM_AUX(AURORA_USB_CHANNELS_7_1_4, AURORA_USB_PERIOD_FRAMES),
                     pcm, sizeof(pcm));
    assert(aurora_realtime_mcu_app_usb_receive(&app, frame, len) == 0);
    assert(ctx.queue_calls == 1 && ctx.muted == 0);

    ctx.sink_counter=48000; ctx.source_counter=48005; ctx.queued_frames=80; clear_tx(&ctx);
    assert(aurora_realtime_mcu_app_send_clock_report(&app, 0) == 0);
    assert(aurora_usb_read_le16(ctx.tx + 6) == AURORA_USB_CLOCK_REPORT);

    aurora_realtime_mcu_app_playback_xrun(&app);
    assert(ctx.muted == 1);
    assert(aurora_transport_get_state(&app.transport) == AURORA_TRANSPORT_XRUN_MUTED);

    puts("realtime-MCU integrated Aurora app tests passed");
    return 0;
}
