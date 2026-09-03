#ifndef AURORA_REALTIME_MCU_HAL_H
#define AURORA_REALTIME_MCU_HAL_H

#include <stddef.h>
#include <stdint.h>

#include "aurora_stm32_audio_app.h"

/*
 * Target-independent boundary between vendor USB/SAI/GPIO callbacks and the
 * single Aurora realtime-MCU application core.
 *
 * This layer deliberately contains no STM32Cube symbols and no concrete pin
 * numbers. Target constants are generated from config/aurora-hardware-target.env
 * into aurora_hw_target_generated.h and consumed by the vendor binding later.
 */
struct aurora_realtime_mcu_hal_io {
    void *ctx;

    /* USB bulk transport toward the S6 gadget. */
    int (*usb_send)(void *ctx, const uint8_t *data, size_t len);

    /* Queue exactly one canonical 40-frame / 12-channel S32LE period. */
    int (*queue_pcm_period)(void *ctx, const uint8_t *pcm, size_t len,
                            uint64_t pts_48k, uint32_t flags);

    /* Fail-closed hardware controls and physical-clock telemetry. */
    void (*set_amplifier_mute)(void *ctx, int muted);
    uint64_t (*sink_sample_counter)(void *ctx);
    uint64_t (*source_sample_counter)(void *ctx);
    uint32_t (*queued_playback_frames)(void *ctx);
};

struct aurora_realtime_mcu_hal {
    struct aurora_stm32_audio_app app;
    struct aurora_transport_io app_io;
    uint64_t next_earc_carrier_frame;
    uint32_t earc_carrier_rate_hz;
    uint8_t carrier_locked;
    uint8_t usb_session_active;
    uint8_t vbus_faulted;
};

void aurora_realtime_mcu_hal_init(
    struct aurora_realtime_mcu_hal *hal,
    const struct aurora_realtime_mcu_hal_io *io,
    const uint8_t expected_layout_hash[32]);

/* USB Host attach/reset/disconnect boundaries. A new session always resets the
 * protocol and capture continuity state before accepting bytes from the S6.
 */
void aurora_realtime_mcu_hal_usb_session_begin(
    struct aurora_realtime_mcu_hal *hal);
void aurora_realtime_mcu_hal_usb_session_end(
    struct aurora_realtime_mcu_hal *hal);
int aurora_realtime_mcu_hal_usb_receive(
    struct aurora_realtime_mcu_hal *hal,
    const uint8_t *data,
    size_t len);

/* eARC receiver clock-lock boundary. The DMA callback supplies only captured
 * S32 slots; carrier-frame accounting remains owned by this one HAL boundary.
 */
int aurora_realtime_mcu_hal_earc_lock(
    struct aurora_realtime_mcu_hal *hal,
    uint32_t carrier_rate_hz);
void aurora_realtime_mcu_hal_earc_unlock(
    struct aurora_realtime_mcu_hal *hal);
int aurora_realtime_mcu_hal_earc_dma_s32_high_words(
    struct aurora_realtime_mcu_hal *hal,
    const uint32_t *slots,
    size_t slot_count,
    uint32_t flags,
    uint8_t *scratch,
    size_t scratch_capacity);

/* Periodic clock telemetry and playback fault boundaries. */
int aurora_realtime_mcu_hal_clock_tick(
    struct aurora_realtime_mcu_hal *hal,
    uint32_t extra_flags);
void aurora_realtime_mcu_hal_playback_xrun(
    struct aurora_realtime_mcu_hal *hal);

/* Protected VBUS switch fault is a hard session boundary. It immediately
 * fails closed and requires a fresh USB session after the electrical fault is
 * cleared by board-specific code.
 */
void aurora_realtime_mcu_hal_vbus_fault(
    struct aurora_realtime_mcu_hal *hal);

#endif /* AURORA_REALTIME_MCU_HAL_H */
