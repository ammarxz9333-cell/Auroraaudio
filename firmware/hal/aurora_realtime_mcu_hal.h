#ifndef AURORA_REALTIME_MCU_HAL_H
#define AURORA_REALTIME_MCU_HAL_H

#include <stddef.h>
#include <stdint.h>

#include "aurora_realtime_mcu_app.h"

/* Target-independent boundary between vendor USB/serial-audio/GPIO callbacks
 * and the single Aurora realtime-MCU application core. Concrete target constants
 * are generated from config/aurora-hardware-target.env. */
struct aurora_realtime_mcu_hal_io {
    void *ctx;
    int (*usb_send)(void *ctx, const uint8_t *data, size_t len);
    int (*queue_pcm_period)(void *ctx, const uint8_t *pcm, size_t len,
                            uint64_t pts_48k, uint32_t flags);
    void (*set_amplifier_mute)(void *ctx, int muted);
    uint64_t (*sink_sample_counter)(void *ctx);
    uint64_t (*source_sample_counter)(void *ctx);
    uint32_t (*queued_playback_frames)(void *ctx);
};

struct aurora_realtime_mcu_hal {
    struct aurora_realtime_mcu_app app;
    struct aurora_transport_io app_io;
    uint64_t next_earc_carrier_frame;
    uint32_t earc_carrier_rate_hz;
    uint8_t carrier_locked;
    uint8_t source_discontinuity_pending;
    uint8_t usb_session_active;
    uint8_t vbus_faulted;
};

void aurora_realtime_mcu_hal_init(
    struct aurora_realtime_mcu_hal *hal,
    const struct aurora_realtime_mcu_hal_io *io,
    const uint8_t expected_layout_hash[32]);
void aurora_realtime_mcu_hal_usb_session_begin(struct aurora_realtime_mcu_hal *hal);
void aurora_realtime_mcu_hal_usb_session_end(struct aurora_realtime_mcu_hal *hal);
int aurora_realtime_mcu_hal_usb_receive(struct aurora_realtime_mcu_hal *hal,
                                        const uint8_t *data, size_t len);
int aurora_realtime_mcu_hal_earc_lock(struct aurora_realtime_mcu_hal *hal,
                                      uint32_t carrier_rate_hz);
void aurora_realtime_mcu_hal_earc_unlock(struct aurora_realtime_mcu_hal *hal);
int aurora_realtime_mcu_hal_earc_dma_s32_high_words(
    struct aurora_realtime_mcu_hal *hal,
    const uint32_t *slots, size_t slot_count, uint32_t flags,
    uint8_t *scratch, size_t scratch_capacity);
int aurora_realtime_mcu_hal_clock_tick(struct aurora_realtime_mcu_hal *hal,
                                       uint32_t extra_flags);
void aurora_realtime_mcu_hal_playback_xrun(struct aurora_realtime_mcu_hal *hal);
void aurora_realtime_mcu_hal_vbus_fault(struct aurora_realtime_mcu_hal *hal);
void aurora_realtime_mcu_hal_vbus_fault_cleared(struct aurora_realtime_mcu_hal *hal);

#endif /* AURORA_REALTIME_MCU_HAL_H */
