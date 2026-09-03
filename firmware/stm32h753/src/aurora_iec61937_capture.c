#include "aurora_iec61937_capture.h"
#include "aurora_transport.h"

int aurora_iec61937_capture_s32_high_words(const uint32_t *slots,
                                           size_t slot_count,
                                           uint8_t *out,
                                           size_t out_capacity,
                                           size_t *out_len)
{
    if (!out_len)
        return -1;
    *out_len = 0;

    if ((!slots && slot_count) || (!out && slot_count))
        return -1;
    if (slot_count > (SIZE_MAX / 2u))
        return -1;

    const size_t needed = slot_count * 2u;
    if (out_capacity < needed)
        return -2;

    for (size_t i = 0; i < slot_count; ++i) {
        uint16_t word = (uint16_t)(slots[i] >> 16);
        out[i * 2u + 0u] = (uint8_t)(word & 0xffu);
        out[i * 2u + 1u] = (uint8_t)(word >> 8);
    }

    *out_len = needed;
    return 0;
}

int aurora_iec61937_capture_pts_48k(uint64_t carrier_frame_counter,
                                   uint32_t carrier_rate_hz,
                                   uint64_t *pts_48k)
{
    if (!pts_48k)
        return -1;

    uint32_t factor;
    switch (carrier_rate_hz) {
    case 48000u:
        factor = 1u;
        break;
    case 96000u:
        factor = 2u;
        break;
    case 192000u:
        factor = 4u;
        break;
    default:
        return -2;
    }

    *pts_48k = carrier_frame_counter / factor;
    return 0;
}

int aurora_iec61937_capture_forward_s32_high_words(
    struct aurora_transport *transport,
    const uint32_t *slots,
    size_t slot_count,
    uint64_t first_carrier_frame,
    uint32_t carrier_rate_hz,
    uint32_t flags,
    uint8_t *scratch,
    size_t scratch_capacity)
{
    if (!transport || (!slots && slot_count) || (!scratch && slot_count))
        return -1;

    /* L/R carrier slots must never be split between Aurora application frames. */
    if ((slot_count & 1u) != 0)
        return -2;

    size_t payload_len = 0;
    int rc = aurora_iec61937_capture_s32_high_words(
        slots, slot_count, scratch, scratch_capacity, &payload_len);
    if (rc != 0)
        return -3;

    if (payload_len == 0)
        return 0;
    if (payload_len > AURORA_USB_MAX_FRAME - AURORA_USB_HEADER_LEN)
        return -4;

    uint64_t pts_48k = 0;
    rc = aurora_iec61937_capture_pts_48k(
        first_carrier_frame, carrier_rate_hz, &pts_48k);
    if (rc != 0)
        return -5;

    return aurora_transport_send_iec61937(
        transport, scratch, payload_len, pts_48k, flags);
}
