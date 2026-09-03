#ifndef AURORA_IEC61937_CAPTURE_H
#define AURORA_IEC61937_CAPTURE_H

#include <stddef.h>
#include <stdint.h>

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

#endif /* AURORA_IEC61937_CAPTURE_H */
