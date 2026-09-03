#include "aurora_realtime_mcu_hal.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "aurora_usb_v1.h"

#define TX_CAPACITY (64u * 1024u)

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
    (void)pts_48k;
    (void)flags;
    if (!ctx || !pcm || len != AURORA_STM32_PCM_PERIOD_BYTES)
        return -1;
    ctx->queue_calls++;
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
    ctx->tx_len = 0u;
    memset(ctx->tx, 0, sizeof(ctx->tx));
}

static uint32_t first_flags_for_kind(const struct mock_ctx *ctx, uint16_t wanted)
{
    size_t off = 0u;

    while (off + AURORA_USB_HEADER_LEN <= ctx->tx_len) {
        const uint8_t *h = ctx->tx + off;
        uint16_t kind = aurora_usb_read_le16(h + 6);
        uint32_t payload_len = aurora_usb_read_le32(h + 24);
        size_t frame_len = AURORA_USB_HEADER_LEN + (size_t)payload_len;
        assert(off + frame_len <= ctx->tx_len);
        if (kind == wanted)
            return aurora_usb_read_le32(h + 8);
        off += frame_len;
    }
    return 0u;
}

static uint64_t first_pts_for_kind(const struct mock_ctx *ctx, uint16_t wanted)
{
    size_t off = 0u;

    while (off + AURORA_USB_HEADER_LEN <= ctx->tx_len) {
        const uint8_t *h = ctx->tx + off;
        uint16_t kind = aurora_usb_read_le16(h + 6);
        uint32_t payload_len = aurora_usb_read_le32(h + 24);
        size_t frame_len = AURORA_USB_HEADER_LEN + (size_t)payload_len;
        assert(off + frame_len <= ctx->tx_len);
        if (kind == wanted)
            return aurora_usb_read_le64(h + 16);
        off += frame_len;
    }
    return UINT64_MAX;
}

int main(void)
{
    struct aurora_realtime_mcu_hal hal;
    struct aurora_realtime_mcu_hal_io io;
    struct mock_ctx ctx;
    uint8_t layout_hash[32];
    uint8_t scratch[64];
    uint32_t slots[4] = {
        0xf8720000u,
        0x4e1f0000u,
        0x00150000u,
        0x00000000u,
    };
    const uint8_t ping_frame[AURORA_USB_HEADER_LEN] = {
        0x41, 0x55, 0x52, 0x41, /* AURA */
        0x01, 0x00,             /* version */
        AURORA_USB_PING, 0x00,  /* kind */
    };

    memset(&ctx, 0, sizeof(ctx));
    memset(&io, 0, sizeof(io));
    memset(layout_hash, 0x5a, sizeof(layout_hash));
    io.ctx = &ctx;
    io.usb_send = mock_usb_send;
    io.queue_pcm_period = mock_queue_pcm;
    io.set_amplifier_mute = mock_set_mute;
    io.sink_sample_counter = mock_sink_counter;
    io.source_sample_counter = mock_source_counter;
    io.queued_playback_frames = mock_queued_frames;

    aurora_realtime_mcu_hal_init(&hal, &io, layout_hash);
    assert(ctx.muted == 1);
    assert(hal.usb_session_active == 0u);
    assert(hal.vbus_faulted == 0u);
    assert(aurora_realtime_mcu_hal_usb_receive(&hal, ping_frame,
                                               sizeof(ping_frame)) == -2);

    assert(aurora_realtime_mcu_hal_earc_lock(&hal, 44100u) == -2);
    assert(aurora_realtime_mcu_hal_earc_lock(&hal, 192000u) == 0);

    /* A fresh USB session is a hard decoder/source discontinuity. */
    aurora_realtime_mcu_hal_usb_session_begin(&hal);
    assert(hal.usb_session_active == 1u);
    tx_reset(&ctx);
    assert(aurora_realtime_mcu_hal_earc_dma_s32_high_words(
               &hal, slots, 4u, 0u, scratch, sizeof(scratch)) == 0);
    assert((first_flags_for_kind(&ctx, AURORA_USB_ENCODED_IEC61937) &
            AURORA_USB_FLAG_DISCONTINUITY) != 0u);
    assert(first_pts_for_kind(&ctx, AURORA_USB_ENCODED_IEC61937) == 0u);

    /* Continuous blocks keep monotonic carrier-derived PTS and no reset flag. */
    tx_reset(&ctx);
    assert(aurora_realtime_mcu_hal_earc_dma_s32_high_words(
               &hal, slots, 4u, 0u, scratch, sizeof(scratch)) == 0);
    assert((first_flags_for_kind(&ctx, AURORA_USB_ENCODED_IEC61937) &
            AURORA_USB_FLAG_DISCONTINUITY) == 0u);
    assert(first_pts_for_kind(&ctx, AURORA_USB_ENCODED_IEC61937) == 0u);

    /* Same-rate unlock/relock must still reset the S6 decoder/render state. */
    aurora_realtime_mcu_hal_earc_unlock(&hal);
    assert(aurora_realtime_mcu_hal_earc_dma_s32_high_words(
               &hal, slots, 4u, 0u, scratch, sizeof(scratch)) == -2);
    assert(aurora_realtime_mcu_hal_earc_lock(&hal, 192000u) == 0);
    tx_reset(&ctx);
    assert(aurora_realtime_mcu_hal_earc_dma_s32_high_words(
               &hal, slots, 4u, 0u, scratch, sizeof(scratch)) == 0);
    assert((first_flags_for_kind(&ctx, AURORA_USB_ENCODED_IEC61937) &
            AURORA_USB_FLAG_DISCONTINUITY) != 0u);

    /* VBUS fault is fail-closed and cannot self-recover into an old session. */
    aurora_realtime_mcu_hal_vbus_fault(&hal);
    assert(hal.vbus_faulted == 1u);
    assert(hal.usb_session_active == 0u);
    assert(ctx.muted == 1);
    assert(aurora_realtime_mcu_hal_earc_dma_s32_high_words(
               &hal, slots, 4u, 0u, scratch, sizeof(scratch)) == -4);
    assert(aurora_realtime_mcu_hal_usb_receive(&hal, ping_frame,
                                               sizeof(ping_frame)) == -2);

    aurora_realtime_mcu_hal_vbus_fault_cleared(&hal);
    assert(hal.vbus_faulted == 0u);
    assert(hal.usb_session_active == 0u);
    assert(aurora_realtime_mcu_hal_usb_receive(&hal, ping_frame,
                                               sizeof(ping_frame)) == -2);

    aurora_realtime_mcu_hal_usb_session_begin(&hal);
    assert(hal.usb_session_active == 1u);

    ctx.sink_counter = 1234u;
    ctx.source_counter = 1200u;
    ctx.queued_frames = 80u;
    tx_reset(&ctx);
    assert(aurora_realtime_mcu_hal_clock_tick(&hal, 0u) == 0);
    assert(first_pts_for_kind(&ctx, AURORA_USB_CLOCK_REPORT) == 1234u);

    aurora_realtime_mcu_hal_playback_xrun(&hal);
    assert(ctx.muted == 1);

    puts("realtime MCU HAL boundary tests passed");
    return 0;
}
