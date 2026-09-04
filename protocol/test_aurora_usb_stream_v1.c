#include "aurora_usb_stream_v1.h"
#include "aurora_usb_v1.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

struct capture {
    unsigned count;
    uint16_t kinds[4];
    size_t lens[4];
};

static int capture_cb(void *opaque, const uint8_t *frame, size_t len)
{
    struct capture *c = opaque;
    assert(c->count < 4);
    assert(len >= AURORA_USB_HEADER_LEN);
    c->kinds[c->count] = aurora_usb_read_le16(frame + 6);
    c->lens[c->count] = len;
    c->count++;
    return 0;
}

static size_t make_frame(uint8_t *dst, uint16_t kind,
                         const uint8_t *payload, uint32_t payload_len,
                         uint32_t sequence)
{
    memset(dst, 0, AURORA_USB_HEADER_LEN + payload_len);
    aurora_usb_write_le32(dst + 0, AURORA_USB_MAGIC_U32);
    aurora_usb_write_le16(dst + 4, AURORA_USB_VERSION);
    aurora_usb_write_le16(dst + 6, kind);
    aurora_usb_write_le32(dst + 12, sequence);
    aurora_usb_write_le32(dst + 24, payload_len);
    if (payload_len)
        memcpy(dst + AURORA_USB_HEADER_LEN, payload, payload_len);
    return AURORA_USB_HEADER_LEN + payload_len;
}

int main(void)
{
    uint8_t storage[4096];
    uint8_t wire[1024];
    struct aurora_usb_stream_v1 stream;
    struct capture c = {0};
    aurora_usb_stream_v1_init(&stream, storage, sizeof(storage));

    static const uint8_t p1[] = {1, 2, 3, 4, 5};
    size_t f1 = make_frame(wire, AURORA_USB_PING, p1, sizeof(p1), 1);

    /* Deliberately split inside magic/header/payload boundaries. */
    assert(aurora_usb_stream_v1_feed(&stream, wire, 1, capture_cb, &c) == 0);
    assert(c.count == 0);
    assert(aurora_usb_stream_v1_feed(&stream, wire + 1, 10, capture_cb, &c) == 0);
    assert(c.count == 0);
    assert(aurora_usb_stream_v1_feed(&stream, wire + 11, f1 - 11, capture_cb, &c) == 0);
    assert(c.count == 1);
    assert(c.kinds[0] == AURORA_USB_PING);
    assert(c.lens[0] == f1);

    /* Deliberately coalesce two complete application frames in one read. */
    size_t f2 = make_frame(wire, AURORA_USB_CLOCK_REPORT, NULL, 0, 2);
    size_t f3 = make_frame(wire + f2, AURORA_USB_PONG, NULL, 0, 3);
    assert(aurora_usb_stream_v1_feed(&stream, wire, f2 + f3, capture_cb, &c) == 0);
    assert(c.count == 3);
    assert(c.kinds[1] == AURORA_USB_CLOCK_REPORT);
    assert(c.kinds[2] == AURORA_USB_PONG);

    /* Fail closed on corrupted magic and reset internal state. */
    size_t bad = make_frame(wire, AURORA_USB_PING, NULL, 0, 4);
    wire[0] = 'X';
    assert(aurora_usb_stream_v1_feed(&stream, wire, bad, capture_cb, &c) ==
           AURORA_USB_STREAM_ERR_MAGIC);
    assert(stream.used == 0);

    /* Oversized declared payload must be rejected before allocation/callback. */
    memset(wire, 0, AURORA_USB_HEADER_LEN);
    aurora_usb_write_le32(wire + 0, AURORA_USB_MAGIC_U32);
    aurora_usb_write_le16(wire + 4, AURORA_USB_VERSION);
    aurora_usb_write_le16(wire + 6, AURORA_USB_PCM_S32LE);
    aurora_usb_write_le32(wire + 24, AURORA_USB_MAX_FRAME);
    assert(aurora_usb_stream_v1_feed(&stream, wire, AURORA_USB_HEADER_LEN,
                                     capture_cb, &c) == AURORA_USB_STREAM_ERR_LENGTH);

    puts("aurora_usb_stream_v1: ok");
    return 0;
}
