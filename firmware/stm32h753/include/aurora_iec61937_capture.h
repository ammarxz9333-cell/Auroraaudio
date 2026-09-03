#ifndef AURORA_IEC61937_CAPTURE_H
#define AURORA_IEC61937_CAPTURE_H

#include <stddef.h>
#include <stdint.h>

struct aurora_transport;

/*
 * Minimal capture-side continuity state. This deliberately tracks only the
 * physical IEC61937 carrier rate; Dolby burst/data-type parsing remains owned
 * by the persistent Omniphony parser on the S6 and is not duplicated here.
 */
struct aurora_iec61937_capture_state {
    uint32_t last_carrier_rate_hz;
    uint8_t have_carrier_rate;
};

void aurora_iec61937_capture_state_init(
    struct aurora_iec61937_capture_state *state);
void aurora_iec61937_capture_state_reset(
    struct aurora_iec61937_capture_state *state);

/*
 * Normalize the HDMI/eARC receiver's left-justified S32 capture slots into the
 * protocol-v1 canonical S16_LE IEC61937 word stream.
 *
 * The validated SiI9437/Lindy reference tap presents the 16-bit IEC word in the
 * high half of each 32-bit capture slot. The low half is padding/unused for this
 * purpose and must never be forwarded to the S6.
 *
 * `slots` is the DMA sample order exactly as received by SAI/I2S (for the DD+
 * reference path: L,R,L,R,... on SD0). Each slot produces two output bytes.
 */
int aurora_iec61937_capture_s32_high_words(const uint32_t *slots,
                                           size_t slot_count,
                                           uint8_t *out,
                                           size_t out_capacity,
                                           size_t *out_len);

/* Convert the physical carrier frame counter to the protocol 48 kHz PTS domain.
 * Supported v1 carrier rates are exact multiples used by the reference path.
 */
int aurora_iec61937_capture_pts_48k(uint64_t carrier_frame_counter,
                                   uint32_t carrier_rate_hz,
                                   uint64_t *pts_48k);

/*
 * Portable DMA-block handoff used by the future STM32 HAL callback:
 *
 *   SAI/I2S S32 slots -> canonical S16_LE IEC61937 -> Aurora USB transport.
 *
 * The caller owns `scratch`; no allocation occurs. `slot_count` must contain
 * complete stereo carrier frames (L,R pairs), so it must be even. The PTS is
 * anchored to the first carrier frame represented by `slots`.
 *
 * This function intentionally does not parse Dolby payloads. Burst boundaries
 * may be split across DMA blocks/Aurora USB frames; Omniphony's persistent
 * IEC61937 parser on the S6 reassembles them.
 */
int aurora_iec61937_capture_forward_s32_high_words(
    struct aurora_transport *transport,
    const uint32_t *slots,
    size_t slot_count,
    uint64_t first_carrier_frame,
    uint32_t carrier_rate_hz,
    uint32_t flags,
    uint8_t *scratch,
    size_t scratch_capacity);

/*
 * Stateful wrapper for the future HAL callback. In addition to caller-supplied
 * source/reset flags, it automatically marks the first block after a physical
 * carrier-rate change as DISCONTINUITY. State is committed only after a
 * successful forward, so rejected blocks cannot poison transition tracking.
 * Same-rate codec/data-type changes are intentionally not guessed here.
 */
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
