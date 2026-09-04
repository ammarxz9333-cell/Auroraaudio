#ifndef AURORA_IEC61937_CAPTURE_H
#define AURORA_IEC61937_CAPTURE_H

#include <stddef.h>
#include <stdint.h>

struct aurora_transport;

/* Physical carrier continuity only. Dolby burst/data-type parsing remains owned
 * by the persistent Omniphony parser on the S6 and is not duplicated here. */
struct aurora_iec61937_capture_state {
    uint32_t last_carrier_rate_hz;
    uint8_t have_carrier_rate;
};

void aurora_iec61937_capture_state_init(
    struct aurora_iec61937_capture_state *state);
void aurora_iec61937_capture_state_reset(
    struct aurora_iec61937_capture_state *state);

int aurora_iec61937_capture_s32_high_words(const uint32_t *slots,
                                           size_t slot_count,
                                           uint8_t *out,
                                           size_t out_capacity,
                                           size_t *out_len);
int aurora_iec61937_capture_pts_48k(uint64_t carrier_frame_counter,
                                   uint32_t carrier_rate_hz,
                                   uint64_t *pts_48k);

/* Portable DMA-block handoff used by the realtime-MCU HAL boundary. The caller
 * owns scratch storage; no allocation occurs. Burst parsing remains on S6. */
int aurora_iec61937_capture_forward_s32_high_words(
    struct aurora_transport *transport,
    const uint32_t *slots,
    size_t slot_count,
    uint64_t first_carrier_frame,
    uint32_t carrier_rate_hz,
    uint32_t flags,
    uint8_t *scratch,
    size_t scratch_capacity);

int aurora_iec61937_capture_forward_stream_block(
    struct aurora_iec61937_capture_state *state,
    struct aurora_transport *transport,
    const uint32_t *slots,
    size_t slot_count,
    uint64_t first_carrier_frame,
    uint32_t carrier_rate_hz,
    uint32_t flags,
    uint8_t *scratch,
    size_t scratch_capacity);

#endif /* AURORA_IEC61937_CAPTURE_H */
