#include "aurora_transport.h"

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
    int queue_should_fail;
    unsigned queue_calls;
    uint64_t sink_counter;
    uint64_t source_counter;
    uint32_t queued_frames;
};

static int mock_usb_send(void *opaque, const uint8_t *data, size_t len)
{
    struct mock_ctx *ctx = opaque;
    if (!ctx || !data || ctx->tx_len + len > sizeof(ctx->tx)) return -1;
    memcpy(ctx->tx + ctx->tx_len, data, len);
    ctx->tx_len += len;
    return 0;
}
static int mock_queue_pcm(void *opaque, const uint8_t *pcm, size_t len,
                          uint64_t pts_48k, uint32_t flags)
{
    struct mock_ctx *ctx = opaque;
    (void)pts_48k; (void)flags;
    if (!ctx || !pcm || len != AURORA_REALTIME_MCU_PCM_PERIOD_BYTES) return -1;
    ctx->queue_calls++;
    if (ctx->queue_should_fail) return -1;
    ctx->queued_frames += AURORA_USB_PERIOD_FRAMES;
    return 0;
}
static void mock_set_mute(void *opaque, int muted) { ((struct mock_ctx *)opaque)->muted = muted ? 1 : 0; }
static uint64_t mock_sink(void *opaque) { return ((struct mock_ctx *)opaque)->sink_counter; }
static uint64_t mock_source(void *opaque) { return ((struct mock_ctx *)opaque)->source_counter; }
static uint32_t mock_queued(void *opaque) { return ((struct mock_ctx *)opaque)->queued_frames; }
static void clear_tx(struct mock_ctx *ctx) { ctx->tx_len = 0; memset(ctx->tx, 0, sizeof(ctx->tx)); }

static size_t make_frame(uint8_t *out, uint16_t kind, uint32_t flags,
                         uint32_t sequence, uint64_t pts, uint32_t aux,
                         const uint8_t *payload, uint32_t payload_len)
{
    size_t total = AURORA_USB_HEADER_LEN + (size_t)payload_len;
    memset(out, 0, total);
    aurora_usb_write_le32(out + 0, AURORA_USB_MAGIC_U32);
    aurora_usb_write_le16(out + 4, AURORA_USB_VERSION);
    aurora_usb_write_le16(out + 6, kind);
    aurora_usb_write_le32(out + 8, flags);
    aurora_usb_write_le32(out + 12, sequence);
    aurora_usb_write_le64(out + 16, pts);
    aurora_usb_write_le32(out + 24, payload_len);
    aurora_usb_write_le32(out + 28, aux);
    if (payload_len) memcpy(out + AURORA_USB_HEADER_LEN, payload, payload_len);
    return total;
}

static void make_config(uint8_t payload[sizeof(struct aurora_usb_config_v1)], const uint8_t hash[32])
{
    memset(payload, 0, sizeof(struct aurora_usb_config_v1));
    aurora_usb_write_le32(payload + 0, AURORA_USB_SAMPLE_RATE_HZ);
    aurora_usb_write_le16(payload + 4, AURORA_USB_PERIOD_FRAMES);
    aurora_usb_write_le16(payload + 6, AURORA_USB_CHANNELS_7_1_4);
    aurora_usb_write_le16(payload + 8, AURORA_USB_PCM_FORMAT_S32LE);
    aurora_usb_write_le16(payload + 10, AURORA_USB_LAYOUT_ID_7_1_4);
    memcpy(payload + 16, hash, 32);
}

int main(void)
{
    struct mock_ctx ctx = {0};
    struct aurora_transport_io io = {0};
    struct aurora_transport t;
    uint8_t hash[32];
    uint8_t config[sizeof(struct aurora_usb_config_v1)];
    static uint8_t frame[FRAME_CAPACITY];
    static uint8_t pcm[AURORA_REALTIME_MCU_PCM_PERIOD_BYTES];
    memset(hash, 0x5a, sizeof(hash));

    io.ctx = &ctx; io.usb_send = mock_usb_send; io.queue_pcm_period = mock_queue_pcm;
    io.set_amplifier_mute = mock_set_mute; io.sink_sample_counter = mock_sink;
    io.source_sample_counter = mock_source; io.queued_playback_frames = mock_queued;
    aurora_transport_init(&t, &io, hash);
    assert(ctx.muted == 1);

    size_t len = make_frame(frame, AURORA_USB_PCM_S32LE, AURORA_USB_FLAG_PTS_VALID, 1, 100,
                            AURORA_USB_PCM_AUX(AURORA_USB_CHANNELS_7_1_4, AURORA_USB_PERIOD_FRAMES),
                            pcm, sizeof(pcm));
    assert(aurora_transport_receive(&t, frame, len) == 0);
    assert(ctx.queue_calls == 0 && ctx.muted == 1);

    clear_tx(&ctx); make_config(config, hash);
    len = make_frame(frame, AURORA_USB_CONFIG, 0, 2, 0, 0, config, sizeof(config));
    assert(aurora_transport_receive(&t, frame, len) == 0);
    assert(aurora_transport_get_state(&t) == AURORA_TRANSPORT_ARMED_MUTED);

    clear_tx(&ctx);
    len = make_frame(frame, AURORA_USB_PCM_S32LE, AURORA_USB_FLAG_PTS_VALID, 3, 4096,
                     AURORA_USB_PCM_AUX(AURORA_USB_CHANNELS_7_1_4, AURORA_USB_PERIOD_FRAMES),
                     pcm, sizeof(pcm));
    assert(aurora_transport_receive(&t, frame, len) == 0);
    assert(ctx.queue_calls == 1 && ctx.muted == 0);

    ctx.queue_should_fail = 1; clear_tx(&ctx);
    assert(aurora_transport_receive(&t, frame, len) == 0);
    assert(aurora_transport_get_state(&t) == AURORA_TRANSPORT_XRUN_MUTED);
    assert(aurora_transport_get_xrun_count(&t) == 1 && ctx.muted == 1);

    aurora_transport_usb_reset(&t);
    assert(aurora_transport_get_state(&t) == AURORA_TRANSPORT_WAIT_CONFIG && ctx.muted == 1);

    ctx.sink_counter = 48000; ctx.source_counter = 47998; ctx.queued_frames = 80;
    clear_tx(&ctx);
    assert(aurora_transport_send_clock_report(&t, 0) == 0);
    assert(aurora_usb_read_le16(ctx.tx + 6) == AURORA_USB_CLOCK_REPORT);

    puts("realtime-MCU Aurora transport tests passed");
    return 0;
}
