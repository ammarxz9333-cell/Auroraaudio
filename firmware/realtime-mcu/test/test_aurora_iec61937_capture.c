#include "aurora_iec61937_capture.h"
#include "aurora_transport.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

struct mock_ctx { uint8_t tx[1024]; size_t tx_len; };
static int mock_usb_send(void *opaque, const uint8_t *data, size_t len)
{
    struct mock_ctx *ctx = opaque;
    if (!ctx || !data || ctx->tx_len + len > sizeof(ctx->tx)) return -1;
    memcpy(ctx->tx + ctx->tx_len, data, len); ctx->tx_len += len; return 0;
}
static void clear_tx(struct mock_ctx *ctx) { ctx->tx_len = 0; memset(ctx->tx, 0, sizeof(ctx->tx)); }

int main(void)
{
    struct mock_ctx ctx = {0};
    struct aurora_transport_io io = {0};
    struct aurora_transport transport;
    struct aurora_iec61937_capture_state state;
    uint8_t hash[32] = {0};
    uint8_t scratch[128];
    const uint32_t slots[] = {
        0xf8721111u, 0x4e1f2222u, 0x00153333u, 0x00064444u,
        0x0b775555u, 0x12346666u, 0x56787777u, 0x00008888u,
    };
    const uint8_t expected[] = {
        0x72,0xf8,0x1f,0x4e,0x15,0x00,0x06,0x00,
        0x77,0x0b,0x34,0x12,0x78,0x56,0x00,0x00,
    };

    io.ctx = &ctx; io.usb_send = mock_usb_send;
    aurora_transport_init(&transport, &io, hash);
    aurora_iec61937_capture_state_init(&state);

    assert(aurora_iec61937_capture_forward_s32_high_words(
        &transport, slots, 8, 400, 192000, 0, scratch, sizeof(scratch)) == 0);
    assert(ctx.tx_len == AURORA_USB_HEADER_LEN + sizeof(expected));
    assert(aurora_usb_read_le16(ctx.tx + 6) == AURORA_USB_ENCODED_IEC61937);
    assert(aurora_usb_read_le64(ctx.tx + 16) == 100);
    assert(memcmp(ctx.tx + AURORA_USB_HEADER_LEN, expected, sizeof(expected)) == 0);

    uint64_t pts = 0;
    assert(aurora_iec61937_capture_pts_48k(1234, 48000, &pts) == 0 && pts == 1234);
    assert(aurora_iec61937_capture_pts_48k(1234, 96000, &pts) == 0 && pts == 617);
    assert(aurora_iec61937_capture_pts_48k(1234, 192000, &pts) == 0 && pts == 308);
    assert(aurora_iec61937_capture_pts_48k(1, 44100, &pts) == -2);

    clear_tx(&ctx);
    assert(aurora_iec61937_capture_forward_stream_block(
        &state, &transport, slots, 8, 400, 192000, 0, scratch, sizeof(scratch)) == 0);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_DISCONTINUITY) == 0);
    clear_tx(&ctx);
    assert(aurora_iec61937_capture_forward_stream_block(
        &state, &transport, slots, 8, 400, 96000, 0, scratch, sizeof(scratch)) == 0);
    assert((aurora_usb_read_le32(ctx.tx + 8) & AURORA_USB_FLAG_DISCONTINUITY) != 0);

    puts("realtime-MCU eARC IEC61937 capture tests passed");
    return 0;
}
