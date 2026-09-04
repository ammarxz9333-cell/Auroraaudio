#include "aurora_realtime_mcu_app.h"

void aurora_realtime_mcu_app_init(
    struct aurora_realtime_mcu_app *app,
    const struct aurora_transport_io *io,
    const uint8_t expected_layout_hash[32])
{
    if (!app)
        return;
    aurora_transport_init(&app->transport, io, expected_layout_hash);
    aurora_iec61937_capture_state_init(&app->capture);
}

void aurora_realtime_mcu_app_usb_reset(struct aurora_realtime_mcu_app *app)
{
    if (!app)
        return;
    aurora_transport_usb_reset(&app->transport);
    aurora_iec61937_capture_state_reset(&app->capture);
}

int aurora_realtime_mcu_app_usb_receive(
    struct aurora_realtime_mcu_app *app,
    const uint8_t *data,
    size_t len)
{
    if (!app)
        return -1;
    return aurora_transport_receive(&app->transport, data, len);
}

int aurora_realtime_mcu_app_earc_dma_s32_high_words(
    struct aurora_realtime_mcu_app *app,
    const uint32_t *slots,
    size_t slot_count,
    uint64_t first_carrier_frame,
    uint32_t carrier_rate_hz,
    uint32_t flags,
    uint8_t *scratch,
    size_t scratch_capacity)
{
    if (!app)
        return -1;
    return aurora_iec61937_capture_forward_stream_block(
        &app->capture, &app->transport, slots, slot_count,
        first_carrier_frame, carrier_rate_hz, flags,
        scratch, scratch_capacity);
}

int aurora_realtime_mcu_app_send_clock_report(
    struct aurora_realtime_mcu_app *app,
    uint32_t extra_flags)
{
    if (!app)
        return -1;
    return aurora_transport_send_clock_report(&app->transport, extra_flags);
}

void aurora_realtime_mcu_app_playback_xrun(
    struct aurora_realtime_mcu_app *app)
{
    if (!app)
        return;
    aurora_transport_report_xrun(&app->transport);
}
