#ifndef AURORA_STM32_AUDIO_APP_H
#define AURORA_STM32_AUDIO_APP_H

#include <stddef.h>
#include <stdint.h>

#include "aurora_iec61937_capture.h"
#include "aurora_transport.h"

/*
 * Hardware-independent integration core for the STM32H753 side of Aurora.
 * Vendor HAL callbacks should terminate here rather than calling transport and
 * capture modules independently. This keeps one owner for continuity/reset
 * state and prevents parallel/duplicated audio paths.
 */
struct aurora_stm32_audio_app {
    struct aurora_transport transport;
    struct aurora_iec61937_capture_state capture;
};

void aurora_stm32_audio_app_init(
    struct aurora_stm32_audio_app *app,
    const struct aurora_transport_io *io,
    const uint8_t expected_layout_hash[32]);

/* USB attach/reset/disconnect boundary. Resets both protocol and capture
 * continuity state so stale source-rate history cannot cross a new session.
 */
void aurora_stm32_audio_app_usb_reset(struct aurora_stm32_audio_app *app);

/* Feed arbitrary bytes received from the Galaxy S6 USB bulk OUT path. */
int aurora_stm32_audio_app_usb_receive(
    struct aurora_stm32_audio_app *app,
    const uint8_t *data,
    size_t len);

/* Feed one complete-stereo-slot eARC/SAI DMA block toward the S6 decoder. */
int aurora_stm32_audio_app_earc_dma_s32_high_words(
    struct aurora_stm32_audio_app *app,
    const uint32_t *slots,
    size_t slot_count,
    uint64_t first_carrier_frame,
    uint32_t carrier_rate_hz,
    uint32_t flags,
    uint8_t *scratch,
    size_t scratch_capacity);

/* Periodic sink/source clock telemetry toward the S6 drift controller. */
int aurora_stm32_audio_app_send_clock_report(
    struct aurora_stm32_audio_app *app,
    uint32_t extra_flags);

/* Realtime playback underrun boundary. Always enters the transport's
 * fail-closed XRUN recovery state and mutes through the configured IO callback.
 */
void aurora_stm32_audio_app_playback_xrun(
    struct aurora_stm32_audio_app *app);

#endif /* AURORA_STM32_AUDIO_APP_H */
