#include "aurora_realtime_mcu_hal.h"

#include <string.h>

#include "aurora_usb_v1.h"

static int carrier_rate_supported(uint32_t rate_hz)
{
    return rate_hz == 48000u || rate_hz == 96000u || rate_hz == 192000u;
}

void aurora_realtime_mcu_hal_init(
    struct aurora_realtime_mcu_hal *hal,
    const struct aurora_realtime_mcu_hal_io *io,
    const uint8_t expected_layout_hash[32])
{
    if (!hal)
        return;
    memset(hal, 0, sizeof(*hal));
    if (io) {
        hal->app_io.ctx = io->ctx;
        hal->app_io.usb_send = io->usb_send;
        hal->app_io.queue_pcm_period = io->queue_pcm_period;
        hal->app_io.set_amplifier_mute = io->set_amplifier_mute;
        hal->app_io.sink_sample_counter = io->sink_sample_counter;
        hal->app_io.source_sample_counter = io->source_sample_counter;
        hal->app_io.queued_playback_frames = io->queued_playback_frames;
    }
    aurora_realtime_mcu_app_init(&hal->app, &hal->app_io, expected_layout_hash);
}

void aurora_realtime_mcu_hal_usb_session_begin(struct aurora_realtime_mcu_hal *hal)
{
    if (!hal) return;
    aurora_realtime_mcu_app_usb_reset(&hal->app);
    hal->usb_session_active = hal->vbus_faulted ? 0u : 1u;
    hal->source_discontinuity_pending = 1u;
}

void aurora_realtime_mcu_hal_usb_session_end(struct aurora_realtime_mcu_hal *hal)
{
    if (!hal) return;
    hal->usb_session_active = 0u;
    hal->source_discontinuity_pending = 1u;
    aurora_realtime_mcu_app_usb_reset(&hal->app);
}

int aurora_realtime_mcu_hal_usb_receive(struct aurora_realtime_mcu_hal *hal,
                                        const uint8_t *data, size_t len)
{
    if (!hal || (!data && len != 0u)) return -1;
    if (!hal->usb_session_active || hal->vbus_faulted) return -2;
    return aurora_realtime_mcu_app_usb_receive(&hal->app, data, len);
}

int aurora_realtime_mcu_hal_earc_lock(struct aurora_realtime_mcu_hal *hal,
                                      uint32_t carrier_rate_hz)
{
    if (!hal) return -1;
    if (!carrier_rate_supported(carrier_rate_hz)) return -2;
    if (hal->carrier_locked && hal->earc_carrier_rate_hz != carrier_rate_hz)
        hal->source_discontinuity_pending = 1u;
    hal->carrier_locked = 1u;
    hal->earc_carrier_rate_hz = carrier_rate_hz;
    hal->next_earc_carrier_frame = 0u;
    return 0;
}

void aurora_realtime_mcu_hal_earc_unlock(struct aurora_realtime_mcu_hal *hal)
{
    if (!hal) return;
    if (hal->carrier_locked) hal->source_discontinuity_pending = 1u;
    hal->carrier_locked = 0u;
    hal->earc_carrier_rate_hz = 0u;
    hal->next_earc_carrier_frame = 0u;
}

int aurora_realtime_mcu_hal_earc_dma_s32_high_words(
    struct aurora_realtime_mcu_hal *hal,
    const uint32_t *slots, size_t slot_count, uint32_t flags,
    uint8_t *scratch, size_t scratch_capacity)
{
    uint32_t effective_flags;
    int rc;
    if (!hal || (!slots && slot_count != 0u) || (!scratch && slot_count != 0u)) return -1;
    if (!hal->carrier_locked) return -2;
    if ((slot_count & 1u) != 0u) return -3;
    if (!hal->usb_session_active || hal->vbus_faulted) return -4;

    effective_flags = flags;
    if (hal->source_discontinuity_pending)
        effective_flags |= AURORA_USB_FLAG_DISCONTINUITY;

    rc = aurora_realtime_mcu_app_earc_dma_s32_high_words(
        &hal->app, slots, slot_count, hal->next_earc_carrier_frame,
        hal->earc_carrier_rate_hz, effective_flags, scratch, scratch_capacity);
    if (rc != 0) return rc;
    if (slot_count != 0u) {
        hal->next_earc_carrier_frame += (uint64_t)(slot_count / 2u);
        hal->source_discontinuity_pending = 0u;
    }
    return 0;
}

int aurora_realtime_mcu_hal_clock_tick(struct aurora_realtime_mcu_hal *hal,
                                       uint32_t extra_flags)
{
    if (!hal) return -1;
    if (!hal->usb_session_active || hal->vbus_faulted) return -2;
    return aurora_realtime_mcu_app_send_clock_report(&hal->app, extra_flags);
}

void aurora_realtime_mcu_hal_playback_xrun(struct aurora_realtime_mcu_hal *hal)
{
    if (hal) aurora_realtime_mcu_app_playback_xrun(&hal->app);
}

void aurora_realtime_mcu_hal_vbus_fault(struct aurora_realtime_mcu_hal *hal)
{
    if (!hal) return;
    hal->vbus_faulted = 1u;
    hal->usb_session_active = 0u;
    hal->source_discontinuity_pending = 1u;
    aurora_realtime_mcu_app_usb_reset(&hal->app);
}

void aurora_realtime_mcu_hal_vbus_fault_cleared(struct aurora_realtime_mcu_hal *hal)
{
    if (hal) hal->vbus_faulted = 0u;
}
