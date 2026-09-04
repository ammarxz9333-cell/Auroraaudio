#define _GNU_SOURCE

#include <errno.h>
#include <math.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#include "aurora_source_manager_v1.h"
#include "aurora_usb_v1.h"

#define DEFAULT_REAL_BRIDGE "/run/aurora/usb-bridge.sock"
#define DEFAULT_HDMI_SOURCE_SOCKET "/run/aurora/hdmi-source.sock"
#define DEFAULT_LOCAL_SOURCE_SOCKET "/run/aurora/local-source.sock"
#define DEFAULT_MANAGER_SOCKET "/run/aurora/source-manager.sock"
#define POLL_TIMEOUT_MS 20
#define SOURCE_IDLE_MS 1000u
#define MAX_REVOKE_FADE_MS 1000u
#define FADE_MS AURORA_SOURCE_REVOKE_FADE_MS
#define FADE_FRAMES ((AURORA_USB_SAMPLE_RATE_HZ * FADE_MS) / 1000u)
#define CONFIG_PAYLOAD_BYTES 48u
#define CONFIG_FRAME_BYTES (AURORA_USB_HEADER_LEN + CONFIG_PAYLOAD_BYTES)
#define PCM_PAYLOAD_BYTES \
    (AURORA_USB_PERIOD_FRAMES * AURORA_USB_CHANNELS_7_1_4 * 4u)

#define SLOT_HDMI 0u
#define SLOT_LOCAL 1u
#define SLOT_COUNT 2u
#define NO_CONFIG_OWNER (-1)

static const uint8_t layout_hash_7_1_4_v1[32] = {
    0x05, 0x06, 0x35, 0x60, 0xd6, 0xc5, 0xc1, 0xb7,
    0xd3, 0x70, 0x96, 0x56, 0xcd, 0x8c, 0x64, 0x4a,
    0x6d, 0x2b, 0x52, 0xf5, 0xe8, 0x13, 0x83, 0x77,
    0x1f, 0x26, 0x32, 0x34, 0x42, 0xd0, 0xa2, 0x44,
};

static volatile sig_atomic_t stop_requested;

struct source_slot {
    uint16_t source_id;
    uint32_t source_format;
    int listen_fd;
    int data_fd;
    int manager_fd;
    uint32_t source_sequence;
    uint8_t manager_registered;
    uint8_t present;
    uint8_t granted;
    uint8_t pending_quiesce;
    uint8_t global_muted;
    uint8_t standby;
    uint8_t force_discontinuity;
    uint8_t configured;
    uint8_t config_awaiting_ack;
    uint8_t config_cached;
    float user_gain;
    float current_gain;
    float target_gain;
    uint32_t ramp_frames_remaining;
    uint64_t last_media_ms;
    uint64_t quiesce_deadline_ms;
    uint8_t config_frame[CONFIG_FRAME_BYTES];
    size_t config_frame_len;
};

struct gate {
    int bridge_fd;
    int config_owner_slot;
    struct source_slot slots[SLOT_COUNT];
    const char *manager_socket;
};

static void on_signal(int signo)
{
    (void)signo;
    stop_requested = 1;
}

static uint16_t read_le16(const uint8_t *p)
{
    return (uint16_t)p[0] | ((uint16_t)p[1] << 8);
}

static uint32_t read_le32(const uint8_t *p)
{
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
           ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static uint64_t read_le64(const uint8_t *p)
{
    return (uint64_t)read_le32(p) | ((uint64_t)read_le32(p + 4) << 32);
}

static void write_le16(uint8_t *p, uint16_t value)
{
    p[0] = (uint8_t)(value & 0xffu);
    p[1] = (uint8_t)(value >> 8);
}

static void write_le32(uint8_t *p, uint32_t value)
{
    p[0] = (uint8_t)(value & 0xffu);
    p[1] = (uint8_t)((value >> 8) & 0xffu);
    p[2] = (uint8_t)((value >> 16) & 0xffu);
    p[3] = (uint8_t)(value >> 24);
}

static void write_le64(uint8_t *p, uint64_t value)
{
    write_le32(p, (uint32_t)value);
    write_le32(p + 4, (uint32_t)(value >> 32));
}

static uint64_t monotonic_ms(void)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0)
        return 0;
    return (uint64_t)ts.tv_sec * 1000u + (uint64_t)ts.tv_nsec / 1000000u;
}

static const char *env_or(const char *key, const char *fallback)
{
    const char *value = getenv(key);
    return (value && *value) ? value : fallback;
}

static void close_fd(int *fd)
{
    if (*fd >= 0) {
        close(*fd);
        *fd = -1;
    }
}

static int send_packet(int fd, const uint8_t *data, size_t len)
{
    if (fd < 0 || (!data && len != 0u))
        return -1;
    ssize_t n = send(fd, data, len, MSG_NOSIGNAL);
    return n == (ssize_t)len ? 0 : -1;
}

static int connect_seqpacket(const char *path)
{
    int fd = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return -1;

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    if (strlen(path) >= sizeof(addr.sun_path)) {
        close(fd);
        errno = ENAMETOOLONG;
        return -1;
    }
    strcpy(addr.sun_path, path);
    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        int saved = errno;
        close(fd);
        errno = saved;
        return -1;
    }
    return fd;
}

static int open_listener(const char *path)
{
    int fd = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return -1;

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    if (strlen(path) >= sizeof(addr.sun_path)) {
        close(fd);
        errno = ENAMETOOLONG;
        return -1;
    }
    strcpy(addr.sun_path, path);
    unlink(path);
    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0 ||
        listen(fd, 1) < 0) {
        int saved = errno;
        close(fd);
        unlink(path);
        errno = saved;
        return -1;
    }
    return fd;
}

static int source_send(struct source_slot *slot, uint16_t kind,
                       uint64_t data0, uint32_t data1, uint32_t data2)
{
    if (!slot || slot->manager_fd < 0)
        return -1;

    uint8_t message[AURORA_SOURCE_MESSAGE_BYTES] = {0};
    write_le32(message + 0, AURORA_SOURCE_MAGIC_U32);
    write_le16(message + 4, AURORA_SOURCE_VERSION);
    write_le16(message + 6, kind);
    write_le16(message + 8, slot->source_id);
    write_le16(message + 10, 0);
    write_le32(message + 12, slot->source_sequence++);
    write_le64(message + 16, data0);
    write_le32(message + 24, data1);
    write_le32(message + 28, data2);
    return send_packet(slot->manager_fd, message, sizeof(message));
}

static void set_target_gain(struct source_slot *slot, float target,
                            uint32_t frames)
{
    slot->target_gain = target;
    slot->ramp_frames_remaining = frames;
    if (frames == 0u)
        slot->current_gain = target;
}

static float audible_target(const struct source_slot *slot)
{
    if (!slot->granted || slot->global_muted || slot->standby)
        return 0.0f;
    return slot->user_gain;
}

static void refresh_target(struct source_slot *slot, uint32_t frames)
{
    set_target_gain(slot, audible_target(slot), frames);
}

static void fail_closed(struct source_slot *slot)
{
    slot->granted = 0;
    slot->pending_quiesce = 0;
    slot->quiesce_deadline_ms = 0;
    slot->configured = 0;
    slot->config_awaiting_ack = 0;
    slot->force_discontinuity = 1;
    set_target_gain(slot, 0.0f, 0);
}

static void reset_usb_state(struct source_slot *slot)
{
    slot->configured = 0;
    slot->config_awaiting_ack = 0;
    slot->force_discontinuity = 1;
}

static int register_with_manager(struct source_slot *slot)
{
    if (source_send(slot, AURORA_SOURCE_REGISTER, 0, 0, 0) < 0)
        return -1;
    slot->manager_registered = 1;
    if (slot->present) {
        if (source_send(slot, AURORA_SOURCE_FORMAT, 0,
                        slot->source_format, 0) < 0 ||
            source_send(slot, AURORA_SOURCE_PRESENT, 0, 0, 0) < 0)
            return -1;
    }
    return 0;
}

static int validate_source_message(const struct source_slot *slot,
                                   const uint8_t *message, size_t len,
                                   uint16_t *kind, uint64_t *data0,
                                   uint32_t *data1)
{
    if (!slot || len != AURORA_SOURCE_MESSAGE_BYTES ||
        read_le32(message + 0) != AURORA_SOURCE_MAGIC_U32 ||
        read_le16(message + 4) != AURORA_SOURCE_VERSION ||
        read_le16(message + 8) != slot->source_id ||
        read_le16(message + 10) != 0)
        return -1;
    *kind = read_le16(message + 6);
    *data0 = read_le64(message + 16);
    *data1 = read_le32(message + 24);
    return 0;
}

static int validate_aurora_frame(uint8_t *frame, size_t len,
                                 uint16_t *kind, uint32_t *flags,
                                 uint8_t **payload, uint32_t *payload_len)
{
    if (!frame || len < AURORA_USB_HEADER_LEN ||
        read_le32(frame + 0) != AURORA_USB_MAGIC_U32 ||
        read_le16(frame + 4) != AURORA_USB_VERSION)
        return -1;

    uint32_t plen = read_le32(frame + 24);
    if ((size_t)plen + AURORA_USB_HEADER_LEN != len)
        return -1;
    *kind = read_le16(frame + 6);
    *flags = read_le32(frame + 8);
    *payload = frame + AURORA_USB_HEADER_LEN;
    *payload_len = plen;
    return 0;
}

static int config_frame_is_valid(const uint8_t *frame, size_t len)
{
    uint16_t kind;
    uint32_t flags, payload_len;
    uint8_t *payload;
    if (validate_aurora_frame((uint8_t *)frame, len, &kind, &flags,
                              &payload, &payload_len) < 0)
        return 0;
    (void)flags;
    if (kind != AURORA_USB_CONFIG || payload_len != CONFIG_PAYLOAD_BYTES)
        return 0;
    if (read_le32(payload + 0) != AURORA_USB_SAMPLE_RATE_HZ ||
        read_le16(payload + 4) != AURORA_USB_PERIOD_FRAMES ||
        read_le16(payload + 6) != AURORA_USB_CHANNELS_7_1_4 ||
        read_le16(payload + 8) != AURORA_USB_PCM_FORMAT_S32LE ||
        read_le16(payload + 10) != AURORA_USB_LAYOUT_ID_7_1_4 ||
        read_le32(payload + 12) != 0 ||
        memcmp(payload + 16, layout_hash_7_1_4_v1,
               sizeof(layout_hash_7_1_4_v1)) != 0)
        return 0;
    return 1;
}

static int cache_config(struct source_slot *slot, const uint8_t *frame,
                        size_t len)
{
    if (!slot || len != CONFIG_FRAME_BYTES || !config_frame_is_valid(frame, len))
        return -1;
    memcpy(slot->config_frame, frame, len);
    slot->config_frame_len = len;
    slot->config_cached = 1;
    slot->configured = 0;
    slot->config_awaiting_ack = 0;
    return 0;
}

static int slot_index(const struct gate *gate, const struct source_slot *slot)
{
    if (!gate || !slot || slot < &gate->slots[0] || slot >= &gate->slots[SLOT_COUNT])
        return NO_CONFIG_OWNER;
    return (int)(slot - &gate->slots[0]);
}

static int send_cached_config(struct gate *gate, struct source_slot *slot)
{
    if (!gate || !slot || gate->bridge_fd < 0 || !slot->config_cached)
        return 0;
    int owner = slot_index(gate, slot);
    if (owner == NO_CONFIG_OWNER)
        return -1;
    if (send_packet(gate->bridge_fd, slot->config_frame,
                    slot->config_frame_len) < 0)
        return -1;
    slot->configured = 0;
    slot->config_awaiting_ack = 1;
    gate->config_owner_slot = owner;
    return 0;
}

static int complete_quiesce(struct source_slot *slot)
{
    if (!slot->pending_quiesce)
        return 0;
    slot->granted = 0;
    slot->pending_quiesce = 0;
    slot->quiesce_deadline_ms = 0;
    slot->configured = 0;
    slot->config_awaiting_ack = 0;
    set_target_gain(slot, 0.0f, 0);
    return source_send(slot, AURORA_SOURCE_QUIESCED, 0, 0, 0);
}

static int handle_manager_message(struct gate *gate, struct source_slot *slot,
                                  const uint8_t *message, size_t len)
{
    uint16_t kind;
    uint64_t data0;
    uint32_t data1;
    if (validate_source_message(slot, message, len, &kind, &data0, &data1) < 0)
        return -1;

    switch (kind) {
    case AURORA_SOURCE_STATUS:
        return 0;
    case AURORA_SOURCE_GRANT:
        slot->granted = 1;
        slot->pending_quiesce = 0;
        slot->quiesce_deadline_ms = 0;
        slot->force_discontinuity = 1;
        refresh_target(slot, FADE_FRAMES);
        return send_cached_config(gate, slot);
    case AURORA_SOURCE_REVOKE: {
        uint32_t fade_ms = data0 > MAX_REVOKE_FADE_MS ?
                           MAX_REVOKE_FADE_MS : (uint32_t)data0;
        uint64_t fade_frames =
            ((uint64_t)AURORA_USB_SAMPLE_RATE_HZ * fade_ms) / 1000u;
        slot->pending_quiesce = 1;
        slot->quiesce_deadline_ms = monotonic_ms() + fade_ms;
        set_target_gain(slot, 0.0f, (uint32_t)fade_frames);
        return 0;
    }
    case AURORA_SOURCE_CONTROL:
        switch (data1) {
        case AURORA_SOURCE_CTRL_MUTE:
            slot->global_muted = data0 != 0;
            refresh_target(slot, FADE_FRAMES);
            return 0;
        case AURORA_SOURCE_CTRL_MASTER_GAIN_MDB: {
            int64_t milli_db = (int64_t)data0;
            if (milli_db > 0)
                milli_db = 0;
            if (milli_db < -80000)
                milli_db = -80000;
            slot->user_gain = powf(10.0f, (float)milli_db / 20000.0f);
            refresh_target(slot, FADE_FRAMES);
            return 0;
        }
        case AURORA_SOURCE_CTRL_STANDBY:
            slot->standby = data0 != 0;
            refresh_target(slot, FADE_FRAMES);
            return 0;
        case AURORA_SOURCE_CTRL_LIPSYNC_FRAMES:
            /* Lip-sync delay belongs upstream in each source DSP path. This
             * final mux intentionally does not duplicate a delay line. */
            return 0;
        default:
            return -1;
        }
    default:
        return -1;
    }
}

static void mark_present(struct source_slot *slot)
{
    slot->last_media_ms = monotonic_ms();
    if (slot->present)
        return;
    slot->present = 1;
    if (slot->manager_fd >= 0 && slot->manager_registered) {
        (void)source_send(slot, AURORA_SOURCE_FORMAT, 0,
                          slot->source_format, 0);
        (void)source_send(slot, AURORA_SOURCE_PRESENT, 0, 0, 0);
    }
}

static void mark_absent(struct source_slot *slot)
{
    if (!slot->present)
        return;
    slot->present = 0;
    if (slot->manager_fd >= 0 && slot->manager_registered)
        (void)source_send(slot, AURORA_SOURCE_ABSENT, 0, 0, 0);
    fail_closed(slot);
}

static int32_t read_s32le(const uint8_t *p)
{
    return (int32_t)read_le32(p);
}

static void write_s32le(uint8_t *p, int32_t value)
{
    write_le32(p, (uint32_t)value);
}

/* 0 = forward, 1 = drop, -1 = malformed/error. */
static int apply_pcm_gate(struct source_slot *slot, uint8_t *frame, size_t len)
{
    uint16_t kind;
    uint32_t flags, payload_len;
    uint8_t *payload;
    if (validate_aurora_frame(frame, len, &kind, &flags,
                              &payload, &payload_len) < 0)
        return -1;
    if (kind != AURORA_USB_PCM_S32LE)
        return 0;
    if (payload_len != PCM_PAYLOAD_BYTES)
        return -1;

    if (slot->source_id == AURORA_SOURCE_LOCAL_MUSIC)
        mark_present(slot);

    if ((!slot->granted && !slot->pending_quiesce) || !slot->configured)
        return 1;

    if (slot->force_discontinuity) {
        write_le32(frame + 8, flags | AURORA_USB_FLAG_DISCONTINUITY);
        slot->force_discontinuity = 0;
    }

    for (uint32_t audio_frame = 0;
         audio_frame < AURORA_USB_PERIOD_FRAMES; ++audio_frame) {
        if (slot->ramp_frames_remaining > 0) {
            slot->current_gain +=
                (slot->target_gain - slot->current_gain) /
                (float)slot->ramp_frames_remaining;
            slot->ramp_frames_remaining--;
            if (slot->ramp_frames_remaining == 0)
                slot->current_gain = slot->target_gain;
        }
        uint32_t base = audio_frame * AURORA_USB_CHANNELS_7_1_4;
        for (uint32_t channel = 0;
             channel < AURORA_USB_CHANNELS_7_1_4; ++channel) {
            uint8_t *sample_bytes = payload + (base + channel) * 4u;
            double scaled =
                (double)read_s32le(sample_bytes) * (double)slot->current_gain;
            if (scaled > 2147483647.0)
                scaled = 2147483647.0;
            if (scaled < -2147483648.0)
                scaled = -2147483648.0;
            write_s32le(sample_bytes, (int32_t)llround(scaled));
        }
    }

    if (slot->pending_quiesce && slot->current_gain == 0.0f &&
        slot->ramp_frames_remaining == 0)
        return complete_quiesce(slot) < 0 ? -1 : 1;
    return 0;
}

static struct source_slot *granted_slot(struct gate *gate)
{
    for (size_t i = 0; i < SLOT_COUNT; ++i) {
        if (gate->slots[i].granted || gate->slots[i].pending_quiesce)
            return &gate->slots[i];
    }
    return NULL;
}

static void close_data_client(struct source_slot *slot)
{
    close_fd(&slot->data_fd);
    mark_absent(slot);
    slot->config_cached = 0;
    slot->config_frame_len = 0;
    reset_usb_state(slot);
}

static void close_all_data_clients(struct gate *gate)
{
    gate->config_owner_slot = NO_CONFIG_OWNER;
    for (size_t i = 0; i < SLOT_COUNT; ++i)
        close_data_client(&gate->slots[i]);
}

static int forward_bridge_frame(struct gate *gate, uint8_t *frame, size_t len)
{
    uint16_t kind;
    uint32_t flags, payload_len;
    uint8_t *payload;
    if (validate_aurora_frame(frame, len, &kind, &flags,
                              &payload, &payload_len) < 0)
        return -1;
    (void)flags;

    struct source_slot *hdmi = &gate->slots[SLOT_HDMI];
    if (kind == AURORA_USB_ENCODED_IEC61937) {
        if (hdmi->data_fd >= 0) {
            mark_present(hdmi);
            if (send_packet(hdmi->data_fd, frame, len) < 0)
                close_data_client(hdmi);
        }
        return 0;
    }

    if (kind == AURORA_USB_CLOCK_REPORT) {
        for (size_t i = 0; i < SLOT_COUNT; ++i) {
            struct source_slot *slot = &gate->slots[i];
            if (slot->data_fd >= 0 &&
                send_packet(slot->data_fd, frame, len) < 0)
                close_data_client(slot);
        }
        return 0;
    }

    if (kind == AURORA_USB_ACK && payload_len == 4u &&
        read_le16(payload + 0) == AURORA_USB_CONFIG &&
        read_le16(payload + 2) == 0u) {
        int owner = gate->config_owner_slot;
        gate->config_owner_slot = NO_CONFIG_OWNER;
        if (owner < 0 || owner >= (int)SLOT_COUNT)
            return 0;
        struct source_slot *slot = &gate->slots[owner];
        if (!slot->config_awaiting_ack)
            return 0;
        slot->config_awaiting_ack = 0;
        slot->configured = 1;
        if (slot->data_fd >= 0 && send_packet(slot->data_fd, frame, len) < 0)
            close_data_client(slot);
        return 0;
    }

    if (kind == AURORA_USB_ERROR && payload_len >= 4u &&
        read_le16(payload + 0) == AURORA_USB_CONFIG) {
        int owner = gate->config_owner_slot;
        gate->config_owner_slot = NO_CONFIG_OWNER;
        if (owner < 0 || owner >= (int)SLOT_COUNT)
            return 0;
        struct source_slot *slot = &gate->slots[owner];
        slot->config_awaiting_ack = 0;
        slot->configured = 0;
        if (slot->data_fd >= 0)
            (void)send_packet(slot->data_fd, frame, len);
        return 0;
    }

    struct source_slot *active = granted_slot(gate);
    if (active && active->data_fd >= 0)
        return send_packet(active->data_fd, frame, len);

    if (kind == AURORA_USB_PONG) {
        for (size_t i = 0; i < SLOT_COUNT; ++i) {
            if (gate->slots[i].data_fd >= 0)
                (void)send_packet(gate->slots[i].data_fd, frame, len);
        }
    }
    return 0;
}

static int source_frame_allowed(uint16_t kind)
{
    return kind == AURORA_USB_CONFIG ||
           kind == AURORA_USB_PCM_S32LE ||
           kind == AURORA_USB_PING;
}

static int handle_source_frame(struct gate *gate, struct source_slot *slot,
                               uint8_t *frame, size_t len)
{
    uint16_t kind;
    uint32_t flags, payload_len;
    uint8_t *payload;
    if (validate_aurora_frame(frame, len, &kind, &flags,
                              &payload, &payload_len) < 0 ||
        !source_frame_allowed(kind))
        return -1;
    (void)flags;
    (void)payload;
    (void)payload_len;

    if (kind == AURORA_USB_CONFIG) {
        if (cache_config(slot, frame, len) < 0)
            return -1;
        if (slot->granted)
            return send_cached_config(gate, slot);
        return 0;
    }

    if (kind == AURORA_USB_PCM_S32LE) {
        int gate_rc = apply_pcm_gate(slot, frame, len);
        if (gate_rc != 0)
            return gate_rc;
        return send_packet(gate->bridge_fd, frame, len) < 0 ? -1 : 0;
    }

    if (!slot->granted)
        return 1;
    return send_packet(gate->bridge_fd, frame, len) < 0 ? -1 : 0;
}

static void init_slot(struct source_slot *slot, uint16_t source_id,
                      uint32_t source_format)
{
    memset(slot, 0, sizeof(*slot));
    slot->source_id = source_id;
    slot->source_format = source_format;
    slot->listen_fd = -1;
    slot->data_fd = -1;
    slot->manager_fd = -1;
    slot->user_gain = 1.0f;
    slot->force_discontinuity = 1;
}

static int connect_manager_for_slot(struct gate *gate,
                                    struct source_slot *slot)
{
    slot->manager_fd = connect_seqpacket(gate->manager_socket);
    if (slot->manager_fd < 0)
        return -1;
    slot->manager_registered = 0;
    slot->source_sequence = 0;
    if (register_with_manager(slot) < 0) {
        close_fd(&slot->manager_fd);
        slot->manager_registered = 0;
        fail_closed(slot);
        return -1;
    }
    return 0;
}

static void handle_manager_disconnect(struct source_slot *slot)
{
    close_fd(&slot->manager_fd);
    slot->manager_registered = 0;
    fail_closed(slot);
}

int main(void)
{
    signal(SIGINT, on_signal);
    signal(SIGTERM, on_signal);
    signal(SIGPIPE, SIG_IGN);

    struct gate gate;
    memset(&gate, 0, sizeof(gate));
    gate.bridge_fd = -1;
    gate.config_owner_slot = NO_CONFIG_OWNER;
    gate.manager_socket = env_or("AURORA_SOURCE_MANAGER_SOCKET",
                                 DEFAULT_MANAGER_SOCKET);

    init_slot(&gate.slots[SLOT_HDMI], AURORA_SOURCE_HDMI_EARC,
              AURORA_SOURCE_FORMAT_IEC61937);
    init_slot(&gate.slots[SLOT_LOCAL], AURORA_SOURCE_LOCAL_MUSIC,
              AURORA_SOURCE_FORMAT_PCM_STEREO);

    const char *bridge_socket = env_or("AURORA_USB_BRIDGE_SOCKET_REAL",
                                       DEFAULT_REAL_BRIDGE);
    const char *hdmi_socket = env_or("AURORA_HDMI_SOURCE_SOCKET",
                                     DEFAULT_HDMI_SOURCE_SOCKET);
    const char *local_socket = env_or("AURORA_LOCAL_SOURCE_SOCKET",
                                      DEFAULT_LOCAL_SOURCE_SOCKET);

    gate.slots[SLOT_HDMI].listen_fd = open_listener(hdmi_socket);
    if (gate.slots[SLOT_HDMI].listen_fd < 0) {
        perror("aurora-source-gate: HDMI listener");
        return 1;
    }
    gate.slots[SLOT_LOCAL].listen_fd = open_listener(local_socket);
    if (gate.slots[SLOT_LOCAL].listen_fd < 0) {
        perror("aurora-source-gate: local listener");
        close_fd(&gate.slots[SLOT_HDMI].listen_fd);
        unlink(hdmi_socket);
        return 1;
    }

    uint8_t *source_buf = malloc(AURORA_USB_MAX_FRAME);
    uint8_t *bridge_buf = malloc(AURORA_USB_MAX_FRAME);
    if (!source_buf || !bridge_buf) {
        perror("aurora-source-gate: buffers");
        free(source_buf);
        free(bridge_buf);
        close_fd(&gate.slots[SLOT_HDMI].listen_fd);
        close_fd(&gate.slots[SLOT_LOCAL].listen_fd);
        unlink(hdmi_socket);
        unlink(local_socket);
        return 1;
    }

    while (!stop_requested) {
        if (gate.bridge_fd < 0)
            gate.bridge_fd = connect_seqpacket(bridge_socket);
        for (size_t i = 0; i < SLOT_COUNT; ++i) {
            if (gate.slots[i].manager_fd < 0)
                (void)connect_manager_for_slot(&gate, &gate.slots[i]);
        }

        struct pollfd p[7];
        p[0] = (struct pollfd){.fd = gate.slots[SLOT_HDMI].listen_fd,
                               .events = POLLIN};
        p[1] = (struct pollfd){.fd = gate.slots[SLOT_HDMI].data_fd,
                               .events = POLLIN};
        p[2] = (struct pollfd){.fd = gate.slots[SLOT_LOCAL].listen_fd,
                               .events = POLLIN};
        p[3] = (struct pollfd){.fd = gate.slots[SLOT_LOCAL].data_fd,
                               .events = POLLIN};
        p[4] = (struct pollfd){.fd = gate.bridge_fd, .events = POLLIN};
        p[5] = (struct pollfd){.fd = gate.slots[SLOT_HDMI].manager_fd,
                               .events = POLLIN};
        p[6] = (struct pollfd){.fd = gate.slots[SLOT_LOCAL].manager_fd,
                               .events = POLLIN};

        int rc = poll(p, 7, POLL_TIMEOUT_MS);
        if (rc < 0) {
            if (errno == EINTR)
                continue;
            break;
        }

        for (size_t i = 0; i < SLOT_COUNT; ++i) {
            int poll_index = i == SLOT_HDMI ? 0 : 2;
            struct source_slot *slot = &gate.slots[i];
            if (p[poll_index].revents & POLLIN) {
                int accepted = accept4(slot->listen_fd, NULL, NULL,
                                       SOCK_CLOEXEC);
                if (accepted >= 0) {
                    if (slot->data_fd >= 0)
                        close(accepted);
                    else
                        slot->data_fd = accepted;
                }
            }
        }

        for (size_t i = 0; i < SLOT_COUNT; ++i) {
            int poll_index = i == SLOT_HDMI ? 5 : 6;
            struct source_slot *slot = &gate.slots[i];
            if (slot->manager_fd >= 0 &&
                (p[poll_index].revents & (POLLHUP | POLLERR | POLLNVAL))) {
                handle_manager_disconnect(slot);
            } else if (slot->manager_fd >= 0 &&
                       (p[poll_index].revents & POLLIN)) {
                uint8_t message[AURORA_SOURCE_MESSAGE_BYTES];
                ssize_t n = recv(slot->manager_fd, message,
                                 sizeof(message), 0);
                if (n <= 0 || handle_manager_message(
                                  &gate, slot, message, (size_t)n) < 0)
                    handle_manager_disconnect(slot);
            }
        }

        if (gate.bridge_fd >= 0 &&
            (p[4].revents & (POLLHUP | POLLERR | POLLNVAL))) {
            close_fd(&gate.bridge_fd);
            close_all_data_clients(&gate);
        } else if (gate.bridge_fd >= 0 && (p[4].revents & POLLIN)) {
            ssize_t n = recv(gate.bridge_fd, bridge_buf,
                             AURORA_USB_MAX_FRAME, 0);
            if (n <= 0 || forward_bridge_frame(
                              &gate, bridge_buf, (size_t)n) < 0) {
                close_fd(&gate.bridge_fd);
                close_all_data_clients(&gate);
            }
        }

        for (size_t i = 0; i < SLOT_COUNT; ++i) {
            int poll_index = i == SLOT_HDMI ? 1 : 3;
            struct source_slot *slot = &gate.slots[i];
            if (slot->data_fd >= 0 &&
                (p[poll_index].revents & (POLLHUP | POLLERR | POLLNVAL))) {
                close_data_client(slot);
            } else if (slot->data_fd >= 0 && gate.bridge_fd >= 0 &&
                       (p[poll_index].revents & POLLIN)) {
                ssize_t n = recv(slot->data_fd, source_buf,
                                 AURORA_USB_MAX_FRAME, 0);
                if (n <= 0) {
                    close_data_client(slot);
                } else {
                    int source_rc = handle_source_frame(
                        &gate, slot, source_buf, (size_t)n);
                    if (source_rc < 0)
                        close_data_client(slot);
                }
            }
        }

        uint64_t now = monotonic_ms();
        for (size_t i = 0; i < SLOT_COUNT; ++i) {
            struct source_slot *slot = &gate.slots[i];
            if (slot->present && slot->last_media_ms != 0u &&
                now > slot->last_media_ms + SOURCE_IDLE_MS)
                mark_absent(slot);

            if (slot->pending_quiesce && slot->quiesce_deadline_ms != 0u &&
                now >= slot->quiesce_deadline_ms) {
                if (complete_quiesce(slot) < 0)
                    handle_manager_disconnect(slot);
            }
        }
    }

    free(source_buf);
    free(bridge_buf);
    for (size_t i = 0; i < SLOT_COUNT; ++i) {
        close_fd(&gate.slots[i].data_fd);
        close_fd(&gate.slots[i].manager_fd);
        close_fd(&gate.slots[i].listen_fd);
    }
    close_fd(&gate.bridge_fd);
    unlink(hdmi_socket);
    unlink(local_socket);
    return 0;
}
