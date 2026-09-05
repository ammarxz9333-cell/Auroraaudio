#ifndef AURORA_SOURCE_MANAGER_V1_H
#define AURORA_SOURCE_MANAGER_V1_H

#include <stdint.h>

#define AURORA_SOURCE_MAGIC_U32 0x30435341u /* "ASC0" */
#define AURORA_SOURCE_VERSION 1u
#define AURORA_SOURCE_MESSAGE_BYTES 32u

#define AURORA_SOURCE_NONE 0u
#define AURORA_SOURCE_HDMI_EARC 1u
#define AURORA_SOURCE_LOCAL_MUSIC 2u
#define AURORA_SOURCE_BLUETOOTH 3u
#define AURORA_SOURCE_NETWORK 4u
#define AURORA_SOURCE_CONTROL_CLIENT 255u

#define AURORA_SOURCE_REGISTER 1u
#define AURORA_SOURCE_PRESENT 2u
#define AURORA_SOURCE_ABSENT 3u
#define AURORA_SOURCE_GRANT 4u
#define AURORA_SOURCE_REVOKE 5u
#define AURORA_SOURCE_QUIESCED 6u
#define AURORA_SOURCE_CONTROL 7u
#define AURORA_SOURCE_FORMAT 8u
#define AURORA_SOURCE_ERROR 9u
#define AURORA_SOURCE_STATUS 10u

#define AURORA_SOURCE_FORMAT_UNKNOWN 0u
#define AURORA_SOURCE_FORMAT_PCM_STEREO 1u
#define AURORA_SOURCE_FORMAT_DDPLUS_5_1 2u
#define AURORA_SOURCE_FORMAT_DDPLUS_JOC 3u
#define AURORA_SOURCE_FORMAT_MAT 4u
#define AURORA_SOURCE_FORMAT_IEC61937 5u

#define AURORA_SOURCE_CTRL_MUTE 1u
#define AURORA_SOURCE_CTRL_MASTER_GAIN_MDB 2u
#define AURORA_SOURCE_CTRL_LIPSYNC_FRAMES 3u
#define AURORA_SOURCE_CTRL_STANDBY 4u
#define AURORA_SOURCE_MAX_LIPSYNC_FRAMES 24000u /* 500 ms at 48 kHz */

#define AURORA_SOURCE_REVOKE_FADE_MS 10u

struct aurora_source_message_v1 {
    uint32_t magic;
    uint16_t version;
    uint16_t kind;
    uint16_t source;
    uint16_t reserved;
    uint32_t sequence;
    uint64_t data0;
    uint32_t data1;
    uint32_t data2;
};

_Static_assert(sizeof(struct aurora_source_message_v1) == AURORA_SOURCE_MESSAGE_BYTES,
               "Aurora source-manager wire message must remain 32 bytes");

#endif /* AURORA_SOURCE_MANAGER_V1_H */
