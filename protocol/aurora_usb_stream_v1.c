#include "aurora_usb_stream_v1.h"

#include <string.h>

uint16_t aurora_usb_read_le16(const uint8_t *p)
{
    return (uint16_t)p[0] | ((uint16_t)p[1] << 8);
}

uint32_t aurora_usb_read_le32(const uint8_t *p)
{
    return (uint32_t)p[0] |
           ((uint32_t)p[1] << 8) |
           ((uint32_t)p[2] << 16) |
           ((uint32_t)p[3] << 24);
}

uint64_t aurora_usb_read_le64(const uint8_t *p)
{
    uint64_t lo = aurora_usb_read_le32(p);
    uint64_t hi = aurora_usb_read_le32(p + 4);
    return lo | (hi << 32);
}

void aurora_usb_write_le16(uint8_t *p, uint16_t v)
{
    p[0] = (uint8_t)(v & 0xffu);
    p[1] = (uint8_t)((v >> 8) & 0xffu);
}

void aurora_usb_write_le32(uint8_t *p, uint32_t v)
{
    p[0] = (uint8_t)(v & 0xffu);
    p[1] = (uint8_t)((v >> 8) & 0xffu);
    p[2] = (uint8_t)((v >> 16) & 0xffu);
    p[3] = (uint8_t)((v >> 24) & 0xffu);
}

void aurora_usb_write_le64(uint8_t *p, uint64_t v)
{
    aurora_usb_write_le32(p, (uint32_t)(v & 0xffffffffu));
    aurora_usb_write_le32(p + 4, (uint32_t)(v >> 32));
}

void aurora_usb_stream_v1_init(struct aurora_usb_stream_v1 *stream,
                               uint8_t *buffer, size_t capacity)
{
    if (!stream)
        return;
    stream->buffer = buffer;
    stream->capacity = capacity;
    stream->used = 0;
}

void aurora_usb_stream_v1_reset(struct aurora_usb_stream_v1 *stream)
{
    if (stream)
        stream->used = 0;
}

static int validate_header(const uint8_t *p, size_t capacity, size_t *frame_len)
{
    if (aurora_usb_read_le32(p) != AURORA_USB_MAGIC_U32)
        return AURORA_USB_STREAM_ERR_MAGIC;
    if (aurora_usb_read_le16(p + 4) != AURORA_USB_VERSION)
        return AURORA_USB_STREAM_ERR_VERSION;

    uint32_t payload_len = aurora_usb_read_le32(p + 24);
    if (payload_len > AURORA_USB_MAX_FRAME - AURORA_USB_HEADER_LEN)
        return AURORA_USB_STREAM_ERR_LENGTH;

    size_t total = (size_t)AURORA_USB_HEADER_LEN + (size_t)payload_len;
    if (total > capacity)
        return AURORA_USB_STREAM_ERR_CAPACITY;

    *frame_len = total;
    return AURORA_USB_STREAM_OK;
}

int aurora_usb_stream_v1_feed(struct aurora_usb_stream_v1 *stream,
                              const uint8_t *data, size_t len,
                              aurora_usb_frame_cb cb, void *ctx)
{
    if (!stream || !stream->buffer || !cb || (!data && len != 0))
        return AURORA_USB_STREAM_ERR_ARGUMENT;
    if (stream->capacity < AURORA_USB_HEADER_LEN ||
        stream->capacity > AURORA_USB_MAX_FRAME)
        return AURORA_USB_STREAM_ERR_CAPACITY;

    while (len > 0) {
        size_t free_space = stream->capacity - stream->used;
        if (free_space == 0) {
            stream->used = 0;
            return AURORA_USB_STREAM_ERR_CAPACITY;
        }

        size_t take = len < free_space ? len : free_space;
        memcpy(stream->buffer + stream->used, data, take);
        stream->used += take;
        data += take;
        len -= take;

        for (;;) {
            if (stream->used < AURORA_USB_HEADER_LEN)
                break;

            size_t frame_len = 0;
            int rc = validate_header(stream->buffer, stream->capacity, &frame_len);
            if (rc != AURORA_USB_STREAM_OK) {
                stream->used = 0;
                return rc;
            }
            if (stream->used < frame_len)
                break;

            if (cb(ctx, stream->buffer, frame_len) != 0) {
                stream->used = 0;
                return AURORA_USB_STREAM_ERR_CALLBACK;
            }

            size_t remaining = stream->used - frame_len;
            if (remaining)
                memmove(stream->buffer, stream->buffer + frame_len, remaining);
            stream->used = remaining;
        }

        if (len > 0 && stream->used == stream->capacity) {
            stream->used = 0;
            return AURORA_USB_STREAM_ERR_CAPACITY;
        }
    }

    return AURORA_USB_STREAM_OK;
}
