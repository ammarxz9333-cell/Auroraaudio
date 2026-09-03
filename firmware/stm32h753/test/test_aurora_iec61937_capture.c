#include "aurora_iec61937_capture.h"
#include "aurora_transport.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

struct mock_ctx {
    uint8_t tx[1024];
    size_t tx_len;
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

static void clear_tx(struct mock_ctx *ctx)
{
    ctx->tx_len = 0;
    memset(ctx->tx, 0, sizeof(ctx->tx));
}

int main(void)
{
    struct mock_ctx ctx;
    struct aurora_transport transport;
    struct aurora_transport_io io;
    struct aurora_iec61937_capture_state capture_state;
    uint8_t layout_hash[32] = {0};
    uint8_t scratch[128];

    memset(&ctx, 0, sizeof(ctx));
    memset(&io, 0, sizeof(io));
    io.ctx = &ctx;
    io.usb_send = mock_usb_send;
    aurora_transport_init(&transport, &io, layout_hash);
    aurora_iec61937_capture_state_init(&capture_state);

    /*
     * High 16 bits carry the IEC61937 word; low halves are deliberately noisy
     * to prove they are discarded. The resulting payload begins with a DD+
     * type-0x15 preamble in canonical S16_LE carrier byte order.
     */
    const uint32_t slots[] = {
        0xf8721111u, 0x4e1f2222u,
        0x00153333u, 0x00064444u,
        0x0b775555u, 0x12346666u,
        0x56787777u, 0x00008888u,
    };
    const uint8_t expected[] = {
        0x72, 0xf8, 0x1f, 0x4e,
        0x15, 0x00, 0x06, 0x00,
        0x77, 0x0b, 0x34, 0x12,
        0x78, 0x56, 0x00, 0x00,
    };

    assert(aurora_iec61937_capture_forward_s32_high_words(
               &transport,
               slots,
               sizeof(slots) / sizeof(slots[0]),
               400,
               192000,
               AURORA_USB_FLAG_DISCONTINUITY,
               scratch,
               sizeof(scratch)) == 0);

    assert(ctx.tx_len == AURORA_USB_HEADER_LEN + sizeof(expected));
    assert(aurora_usb_read_le32(ctx.tx + 0) == AURORA_USB_MAGIC_U32);
    assert(aurora_usb_read_le16(ctx.tx + 4) == AURORA_USB_VERSION);
    assert(aurora_usb_read_le16(ctx.tx + 6) == AURORA_USB_ENCODED_IEC61937);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_PTS_VALID) != 0);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_DISCONTINUITY) != 0);
    assert(aurora_usb_read_le64(ctx.tx + 16) == 100);
    assert(aurora_usb_read_le32(ctx.tx + 24) == sizeof(expected));
    assert(memcmp(ctx.tx + AURORA_USB_HEADER_LEN, expected, sizeof(expected)) == 0);

    /* PTS conversion stays in the common 48 kHz protocol clock domain. */
    uint64_t pts = 0;
    assert(aurora_iec61937_capture_pts_48k(1234, 48000, &pts) == 0);
    assert(pts == 1234);
    assert(aurora_iec61937_capture_pts_48k(1234, 96000, &pts) == 0);
    assert(pts == 617);
    assert(aurora_iec61937_capture_pts_48k(1234, 192000, &pts) == 0);
    assert(pts == 308);
    assert(aurora_iec61937_capture_pts_48k(1, 44100, &pts) == -2);

    /* Never forward half of an L/R carrier frame. */
    const size_t before = ctx.tx_len;
    assert(aurora_iec61937_capture_forward_s32_high_words(
               &transport, slots, 3, 0, 192000, 0,
               scratch, sizeof(scratch)) == -2);
    assert(ctx.tx_len == before);

    /* Scratch capacity and carrier-rate errors fail before USB transmission. */
    assert(aurora_iec61937_capture_forward_s32_high_words(
               &transport, slots, 8, 0, 192000, 0,
               scratch, 4) == -3);
    assert(ctx.tx_len == before);
    assert(aurora_iec61937_capture_forward_s32_high_words(
               &transport, slots, 8, 0, 44100, 0,
               scratch, sizeof(scratch)) == -5);
    assert(ctx.tx_len == before);

    /*
     * Stateful capture adds DISCONTINUITY exactly when the physical carrier
     * rate changes. The first valid block after init/reset is not mislabeled.
     */
    clear_tx(&ctx);
    assert(aurora_iec61937_capture_forward_stream_block(
               &capture_state, &transport, slots, 8,
               400, 192000, 0, scratch, sizeof(scratch)) == 0);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_DISCONTINUITY) == 0);
    assert(capture_state.have_carrier_rate == 1);
    assert(capture_state.last_carrier_rate_hz == 192000u);

    clear_tx(&ctx);
    assert(aurora_iec61937_capture_forward_stream_block(
               &capture_state, &transport, slots, 8,
               400, 96000, 0, scratch, sizeof(scratch)) == 0);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_DISCONTINUITY) != 0);
    assert(aurora_usb_read_le64(ctx.tx + 16) == 200);
    assert(capture_state.last_carrier_rate_hz == 96000u);

    aurora_iec61937_capture_state_reset(&capture_state);
    clear_tx(&ctx);
    assert(aurora_iec61937_capture_forward_stream_block(
               &capture_state, &transport, slots, 8,
               400, 96000, 0, scratch, sizeof(scratch)) == 0);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_DISCONTINUITY) == 0);

    /* Rejected rates must not become the remembered continuity state. */
    aurora_iec61937_capture_state_reset(&capture_state);
    clear_tx(&ctx);
    assert(aurora_iec61937_capture_forward_stream_block(
               &capture_state, &transport, slots, 8,
               0, 44100, 0, scratch, sizeof(scratch)) == -5);
    assert(capture_state.have_carrier_rate == 0);
    assert(ctx.tx_len == 0);

    puts("STM32 eARC IEC61937 capture-to-USB tests passed");
    return 0;
}
