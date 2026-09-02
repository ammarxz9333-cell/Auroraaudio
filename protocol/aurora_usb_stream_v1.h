#ifndef AURORA_USB_STREAM_V1_H
#define AURORA_USB_STREAM_V1_H

#include <stddef.h>
#include <stdint.h>

#include "aurora_usb_v1.h"

enum aurora_usb_stream_result {
    AURORA_USB_STREAM_OK = 0,
    AURORA_USB_STREAM_ERR_ARGUMENT = -1,
    AURORA_USB_STREAM_ERR_CAPACITY = -2,
    AURORA_USB_STREAM_ERR_MAGIC = -3,
    AURORA_USB_STREAM_ERR_VERSION = -4,
    AURORA_USB_STREAM_ERR_LENGTH = -5,
    AURORA_USB_STREAM_ERR_CALLBACK = -6,
};

struct aurora_usb_stream_v1 {
    uint8_t *buffer;
    size_t capacity;
    size_t used;
};

typedef int (*aurora_usb_frame_cb)(void *ctx, const uint8_t *frame, size_t len);

void aurora_usb_stream_v1_init(struct aurora_usb_stream_v1 *stream,
                               uint8_t *buffer, size_t capacity);
void aurora_usb_stream_v1_reset(struct aurora_usb_stream_v1 *stream);

int aurora_usb_stream_v1_feed(struct aurora_usb_stream_v1 *stream,
                              const uint8_t *data, size_t len,
                              aurora_usb_frame_cb cb, void *ctx);

uint16_t aurora_usb_read_le16(const uint8_t *p);
uint32_t aurora_usb_read_le32(const uint8_t *p);
uint64_t aurora_usb_read_le64(const uint8_t *p);
void aurora_usb_write_le16(uint8_t *p, uint16_t v);
void aurora_usb_write_le32(uint8_t *p, uint32_t v);
void aurora_usb_write_le64(uint8_t *p, uint64_t v);

#endif /* AURORA_USB_STREAM_V1_H */
