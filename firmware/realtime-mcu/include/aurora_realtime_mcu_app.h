#ifndef AURORA_REALTIME_MCU_APP_H
#define AURORA_REALTIME_MCU_APP_H

#include <stddef.h>
#include <stdint.h>

#include "aurora_iec61937_capture.h"
#include "aurora_transport.h"

/* Hardware-independent integration core. Target-specific HAL callbacks terminate
 * here instead of calling capture and transport independently. */
struct aurora_realtime_mcu_app {
    struct aurora_transport transport;
    struct aurora_iec61937_capture_state capture;
};

void aurora_realtime_mcu_app_init(
    struct aurora_realtime_mcu_app *app,
    const struct aurora_transport_io *io,
    const uint8_t expected_layout_hash[32]);
void aurora_realtime_mcu_app_usb_reset(struct aurora_realtime_mcu_app *app);
int aurora_realtime_mcu_app_usb_receive(
    struct aurora_realtime_mcu_app *app,
    const uint8_t *data,
    size_t len);
int aurora_realtime_mcu_app_earc_dma_s32_high_words(
    struct aurora_realtime_mcu_app *app,
    const uint32_t *slots,
    size_t slot_count,
    uint64_t first_carrier_frame,
    uint32_t carrier_rate_hz,
    uint32_t flags,
    uint8_t *scratch,
    size_t scratch_capacity);
int aurora_realtime_mcu_app_send_clock_report(
    struct aurora_realtime_mcu_app *app,
    uint32_t extra_flags);
void aurora_realtime_mcu_app_playback_xrun(
    struct aurora_realtime_mcu_app *app);

#endif /* AURORA_REALTIME_MCU_APP_H */
