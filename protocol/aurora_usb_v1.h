#ifndef AURORA_USB_V1_H
#define AURORA_USB_V1_H

#include <stdint.h>

#define AURORA_USB_MAGIC_U32 0x30525541u /* "AUR0" on little-endian wire */
#define AURORA_USB_VERSION 1u
#define AURORA_USB_HEADER_LEN 32u
#define AURORA_USB_MAX_FRAME (256u * 1024u)

#define AURORA_USB_SAMPLE_RATE_HZ 48000u
#define AURORA_USB_PERIOD_FRAMES 40u
#define AURORA_USB_CHANNELS_7_1_4 12u
#define AURORA_USB_PCM_FORMAT_S32LE 1u
#define AURORA_USB_LAYOUT_ID_7_1_4 1u
#define AURORA_USB_LAYOUT_HASH_7_1_4_V1_BYTES { \
    0x05, 0x06, 0x35, 0x60, 0xd6, 0xc5, 0xc1, 0xb7, \
    0xd3, 0x70, 0x96, 0x56, 0xcd, 0x8c, 0x64, 0x4a, \
    0x6d, 0x2b, 0x52, 0xf5, 0xe8, 0x13, 0x83, 0x77, \
    0x1f, 0x26, 0x32, 0x34, 0x42, 0xd0, 0xa2, 0x44  \
}

#define AURORA_USB_FLAG_PTS_VALID      (1u << 0)
#define AURORA_USB_FLAG_DISCONTINUITY  (1u << 1)
#define AURORA_USB_FLAG_END_OF_STREAM  (1u << 2)
#define AURORA_USB_FLAG_XRUN_RECOVERY  (1u << 3)

enum aurora_usb_kind_v1 {
    AURORA_USB_ENCODED_IEC61937 = 1,
    AURORA_USB_PCM_S32LE = 2,
    AURORA_USB_CLOCK_REPORT = 3,
    AURORA_USB_CONFIG = 4,
    AURORA_USB_ACK = 5,
    AURORA_USB_ERROR = 6,
    AURORA_USB_PING = 7,
    AURORA_USB_PONG = 8,
};

#if defined(__GNUC__) || defined(__clang__)
#define AURORA_PACKED __attribute__((packed))
#else
#define AURORA_PACKED
#endif

struct AURORA_PACKED aurora_usb_header_v1 {
    uint32_t magic;
    uint16_t version;
    uint16_t kind;
    uint32_t flags;
    uint32_t sequence;
    uint64_t pts_48k;
    uint32_t payload_len;
    uint32_t aux;
};

struct AURORA_PACKED aurora_usb_clock_report_v1 {
    uint64_t sink_sample_counter;
    uint64_t source_sample_counter;
    uint32_t queued_playback_frames;
    uint32_t capture_flags;
};

struct AURORA_PACKED aurora_usb_config_v1 {
    uint32_t sample_rate;
    uint16_t period_frames;
    uint16_t channels;
    uint16_t pcm_format;
    uint16_t layout_id;
    uint32_t reserved;
    uint8_t layout_hash[32];
};

#define AURORA_USB_PCM_AUX(channels, frames) \
    ((((uint32_t)(channels)) << 16) | ((uint32_t)(frames) & 0xffffu))
#define AURORA_USB_PCM_AUX_CHANNELS(aux) ((uint16_t)((uint32_t)(aux) >> 16))
#define AURORA_USB_PCM_AUX_FRAMES(aux) ((uint16_t)((uint32_t)(aux) & 0xffffu))

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(struct aurora_usb_header_v1) == 32, "Aurora USB header must be 32 bytes");
_Static_assert(sizeof(struct aurora_usb_clock_report_v1) == 24, "Aurora CLOCK_REPORT must be 24 bytes");
_Static_assert(sizeof(struct aurora_usb_config_v1) == 48, "Aurora CONFIG must be 48 bytes");
#endif

#endif /* AURORA_USB_V1_H */
