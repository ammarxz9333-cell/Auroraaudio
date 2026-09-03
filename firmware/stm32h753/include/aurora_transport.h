#ifndef AURORA_STM32_TRANSPORT_H
#define AURORA_STM32_TRANSPORT_H

#include <stddef.h>
#include <stdint.h>

#include "aurora_usb_stream_v1.h"
#include "aurora_usb_v1.h"

#define AURORA_STM32_PCM_PERIOD_BYTES \
    (AURORA_USB_PERIOD_FRAMES * AURORA_USB_CHANNELS_7_1_4 * 4u)
#define AURORA_STM32_RX_CAPACITY (16u * 1024u)

enum aurora_transport_state {
    AURORA_TRANSPORT_WAIT_CONFIG = 0,
    AURORA_TRANSPORT_ARMED_MUTED = 1,
    AURORA_TRANSPORT_STREAMING = 2,
    AURORA_TRANSPORT_XRUN_MUTED = 3,
};

struct aurora_transport_io {
    void *ctx;

    /* Sends bytes toward Galaxy S6 EP1 OUT. The transport may call this more
     * than once per Aurora application frame; the S6 stream parser reassembles
     * the byte stream using header.payload_len. Return 0 on success. */
    int (*usb_send)(void *ctx, const uint8_t *data, size_t len);

    /* Queues exactly one 40-frame / 12-channel / S32LE playback period into
     * the realtime SAI/TDM path. Return 0 only if ownership was accepted. */
    int (*queue_pcm_period)(void *ctx, const uint8_t *pcm, size_t len,
                            uint64_t pts_48k, uint32_t flags);

    /* Hardware amplifier mute. true/nonzero must be the power-on-safe state. */
    void (*set_amplifier_mute)(void *ctx, int muted);

    uint64_t (*sink_sample_counter)(void *ctx);
    uint64_t (*source_sample_counter)(void *ctx);
    uint32_t (*queued_playback_frames)(void *ctx);
};

struct aurora_transport {
    struct aurora_transport_io io;
    struct aurora_usb_stream_v1 rx_stream;
    uint8_t rx_storage[AURORA_STM32_RX_CAPACITY];
    uint8_t expected_layout_hash[32];
    uint32_t tx_sequence;
    uint32_t xrun_count;
    enum aurora_transport_state state;
    int configured;
};

void aurora_transport_init(struct aurora_transport *t,
                           const struct aurora_transport_io *io,
                           const uint8_t expected_layout_hash[32]);

/* Call on USB device attach/reset/disconnect before accepting new stream data. */
void aurora_transport_usb_reset(struct aurora_transport *t);

/* Feed arbitrary chunks read from S6 EP2 IN. Split/coalesced USB reads are OK. */
int aurora_transport_receive(struct aurora_transport *t,
                             const uint8_t *data, size_t len);

/* STM32 capture path -> S6 decoder. pts_48k is the capture sample-clock PTS. */
int aurora_transport_send_iec61937(struct aurora_transport *t,
                                   const uint8_t *payload, size_t len,
                                   uint64_t pts_48k, uint32_t flags);

int aurora_transport_send_clock_report(struct aurora_transport *t,
                                       uint32_t extra_flags);

/* Realtime playback path calls this immediately on an underrun. */
void aurora_transport_report_xrun(struct aurora_transport *t);

enum aurora_transport_state aurora_transport_get_state(
    const struct aurora_transport *t);
uint32_t aurora_transport_get_xrun_count(const struct aurora_transport *t);

#endif /* AURORA_STM32_TRANSPORT_H */
