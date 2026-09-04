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
#define DEFAULT_SOURCE_SOCKET "/run/aurora/hdmi-source.sock"
#define DEFAULT_LOCAL_SOURCE_SOCKET "/run/aurora/local-source.sock"
#define DEFAULT_MANAGER_SOCKET "/run/aurora/source-manager.sock"
#define POLL_TIMEOUT_MS 50
#define SOURCE_IDLE_MS 1000u
#define MAX_REVOKE_FADE_MS 1000u
#define FADE_MS AURORA_SOURCE_REVOKE_FADE_MS
#define FADE_FRAMES ((AURORA_USB_SAMPLE_RATE_HZ * FADE_MS) / 1000u)

static volatile sig_atomic_t stop_requested;

struct gate {
    int bridge_fd;
    int source_listen_fd;
    int source_fd;
    int local_listen_fd;
    int local_fd;
    int manager_fd;
    uint32_t source_sequence;
    uint8_t manager_registered;
    uint8_t source_present;
    uint8_t granted;
    uint8_t pending_quiesce;
    uint8_t global_muted;
    uint8_t standby;
    uint8_t force_discontinuity;
    float user_gain;
    float current_gain;
    float target_gain;
    uint32_t ramp_frames_remaining;
    uint64_t last_encoded_ms;
    uint64_t quiesce_deadline_ms;
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

static void write_le16(uint8_t *p, uint16_t v)
{
    p[0] = (uint8_t)(v & 0xffu);
    p[1] = (uint8_t)(v >> 8);
}

static void write_le32(uint8_t *p, uint32_t v)
{
    p[0] = (uint8_t)(v & 0xffu);
    p[1] = (uint8_t)((v >> 8) & 0xffu);
    p[2] = (uint8_t)((v >> 16) & 0xffu);
    p[3] = (uint8_t)(v >> 24);
}

static void write_le64(uint8_t *p, uint64_t v)
{
    write_le32(p, (uint32_t)v);
    write_le32(p + 4, (uint32_t)(v >> 32));
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

static void close_fd(int *fd)
{
    if (*fd >= 0) {
        close(*fd);
        *fd = -1;
    }
}

static int send_packet(int fd, const uint8_t *frame, size_t len)
{
    if (fd < 0)
        return -1;
    ssize_t n = send(fd, frame, len, MSG_NOSIGNAL);
    return n == (ssize_t)len ? 0 : -1;
}

static int source_send(struct gate *g, uint16_t kind, uint64_t data0,
                       uint32_t data1, uint32_t data2)
{
    if (g->manager_fd < 0)
        return -1;
    uint8_t message[AURORA_SOURCE_MESSAGE_BYTES] = {0};
    write_le32(message + 0, AURORA_SOURCE_MAGIC_U32);
    write_le16(message + 4, AURORA_SOURCE_VERSION);
    write_le16(message + 6, kind);
    write_le16(message + 8, AURORA_SOURCE_HDMI_EARC);
    write_le16(message + 10, 0);
    write_le32(message + 12, g->source_sequence++);
    write_le64(message + 16, data0);
    write_le32(message + 24, data1);
    write_le32(message + 28, data2);
    ssize_t n = send(g->manager_fd, message, sizeof(message), MSG_NOSIGNAL);
    return n == (ssize_t)sizeof(message) ? 0 : -1;
}

static void set_target_gain(struct gate *g, float target, uint32_t frames)
{
    g->target_gain = target;
    g->ramp_frames_remaining = frames;
    if (frames == 0)
        g->current_gain = target;
}

static float audible_target(const struct gate *g)
{
    if (!g->granted || g->global_muted || g->standby)
        return 0.0f;
    return g->user_gain;
}

static void refresh_target(struct gate *g, uint32_t frames)
{
    set_target_gain(g, audible_target(g), frames);
}

static void fail_closed(struct gate *g)
{
    g->granted = 0;
    g->pending_quiesce = 0;
    g->quiesce_deadline_ms = 0;
    g->force_discontinuity = 1;
    set_target_gain(g, 0.0f, 0);
}

static int complete_quiesce(struct gate *g)
{
    if (!g->pending_quiesce)
        return 0;
    g->granted = 0;
    g->pending_quiesce = 0;
    g->quiesce_deadline_ms = 0;
    set_target_gain(g, 0.0f, 0);
    return source_send(g, AURORA_SOURCE_QUIESCED, 0, 0, 0);
}

static int register_with_manager(struct gate *g)
{
    if (source_send(g, AURORA_SOURCE_REGISTER, 0, 0, 0) < 0)
        return -1;
    g->manager_registered = 1;
    if (g->source_present) {
        if (source_send(g, AURORA_SOURCE_FORMAT, 0,
                        AURORA_SOURCE_FORMAT_IEC61937, 0) < 0 ||
            source_send(g, AURORA_SOURCE_PRESENT, 0, 0, 0) < 0)
            return -1;
    }
    return 0;
}

static int validate_source_message(const uint8_t *message, size_t len,
                                   uint16_t *kind, uint64_t *data0,
                                   uint32_t *data1)
{
    if (len != AURORA_SOURCE_MESSAGE_BYTES ||
        read_le32(message + 0) != AURORA_SOURCE_MAGIC_U32 ||
        read_le16(message + 4) != AURORA_SOURCE_VERSION ||
        read_le16(message + 8) != AURORA_SOURCE_HDMI_EARC ||
        read_le16(message + 10) != 0)
        return -1;
    *kind = read_le16(message + 6);
    *data0 = read_le64(message + 16);
    *data1 = read_le32(message + 24);
    return 0;
}

static int handle_manager_message(struct gate *g, const uint8_t *message, size_t len)
{
    uint16_t kind;
    uint64_t data0;
    uint32_t data1;
    if (validate_source_message(message, len, &kind, &data0, &data1) < 0)
        return -1;

    switch (kind) {
    case AURORA_SOURCE_STATUS:
        return 0;
    case AURORA_SOURCE_GRANT:
        g->granted = 1;
        g->pending_quiesce = 0;
        g->quiesce_deadline_ms = 0;
        g->force_discontinuity = 1;
        refresh_target(g, FADE_FRAMES);
        return 0;
    case AURORA_SOURCE_REVOKE: {
        uint32_t fade_ms = data0 > MAX_REVOKE_FADE_MS ?
                           MAX_REVOKE_FADE_MS : (uint32_t)data0;
        uint64_t fade_frames = ((uint64_t)AURORA_USB_SAMPLE_RATE_HZ * fade_ms) / 1000u;
        g->pending_quiesce = 1;
        g->quiesce_deadline_ms = monotonic_ms() + fade_ms;
        set_target_gain(g, 0.0f, (uint32_t)fade_frames);
        return 0;
    }
    case AURORA_SOURCE_CONTROL:
        switch (data1) {
        case AURORA_SOURCE_CTRL_MUTE:
            g->global_muted = data0 != 0;
            refresh_target(g, FADE_FRAMES);
            return 0;
        case AURORA_SOURCE_CTRL_MASTER_GAIN_MDB: {
            int64_t milli_db = (int64_t)data0;
            if (milli_db > 0)
                milli_db = 0;
            if (milli_db < -80000)
                milli_db = -80000;
            g->user_gain = powf(10.0f, (float)milli_db / 20000.0f);
            refresh_target(g, FADE_FRAMES);
            return 0;
        }
        case AURORA_SOURCE_CTRL_STANDBY:
            g->standby = data0 != 0;
            refresh_target(g, FADE_FRAMES);
            return 0;
        case AURORA_SOURCE_CTRL_LIPSYNC_FRAMES:
            /* The actual delay line remains upstream in the Aurora postprocessor.
             * This final post-limiter gate must never duplicate lip-sync delay. */
            return 0;
        default:
            return -1;
        }
    default:
        return -1;
    }
}

static int validate_aurora_frame(uint8_t *frame, size_t len,
                                 uint16_t *kind, uint32_t *flags,
                                 uint8_t **payload, uint32_t *payload_len)
{
    if (len < AURORA_USB_HEADER_LEN ||
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

static int32_t read_s32le(const uint8_t *p)
{
    return (int32_t)read_le32(p);
}

static void write_s32le(uint8_t *p, int32_t value)
{
    write_le32(p, (uint32_t)value);
}

/* Returns 0 to forward the frame, 1 to drop an inactive HDMI PCM period, and
 * -1 on invalid input. Dropping inactive PCM is mandatory once multiple source
 * data clients share the single FunctionFS backend: zero periods from an
 * inactive source would otherwise overwrite the active source's timeline. */
static int apply_pcm_gate(struct gate *g, uint8_t *frame, size_t len)
{
    uint16_t kind;
    uint32_t flags;
    uint8_t *payload;
    uint32_t payload_len;
    if (validate_aurora_frame(frame, len, &kind, &flags, &payload, &payload_len) < 0)
        return -1;
    if (kind != AURORA_USB_PCM_S32LE)
        return 0;
    if (payload_len != AURORA_USB_PERIOD_FRAMES * AURORA_USB_CHANNELS_7_1_4 * 4u)
        return -1;
    if (!g->granted && !g->pending_quiesce)
        return 1;

    if (g->force_discontinuity) {
        write_le32(frame + 8, flags | AURORA_USB_FLAG_DISCONTINUITY);
        g->force_discontinuity = 0;
    }

    for (uint32_t audio_frame = 0; audio_frame < AURORA_USB_PERIOD_FRAMES; ++audio_frame) {
        if (g->ramp_frames_remaining > 0) {
            g->current_gain += (g->target_gain - g->current_gain) /
                               (float)g->ramp_frames_remaining;
            g->ramp_frames_remaining--;
            if (g->ramp_frames_remaining == 0)
                g->current_gain = g->target_gain;
        }
        uint32_t base = audio_frame * AURORA_USB_CHANNELS_7_1_4;
        for (uint32_t channel = 0; channel < AURORA_USB_CHANNELS_7_1_4; ++channel) {
            uint8_t *sample_bytes = payload + (base + channel) * 4u;
            double scaled = (double)read_s32le(sample_bytes) * (double)g->current_gain;
            if (scaled > 2147483647.0)
                scaled = 2147483647.0;
            if (scaled < -2147483648.0)
                scaled = -2147483648.0;
            write_s32le(sample_bytes, (int32_t)llround(scaled));
        }
    }

    if (g->pending_quiesce && g->current_gain == 0.0f &&
        g->ramp_frames_remaining == 0)
        return complete_quiesce(g) < 0 ? -1 : 1;
    return 0;
}

static void mark_source_present(struct gate *g)
{
    g->last_encoded_ms = monotonic_ms();
    if (g->source_present)
        return;
    g->source_present = 1;
    if (g->manager_fd >= 0 && g->manager_registered) {
        (void)source_send(g, AURORA_SOURCE_FORMAT, 0,
                          AURORA_SOURCE_FORMAT_IEC61937, 0);
        (void)source_send(g, AURORA_SOURCE_PRESENT, 0, 0, 0);
    }
}

static void mark_source_absent(struct gate *g)
{
    if (!g->source_present)
        return;
    g->source_present = 0;
    if (g->manager_fd >= 0 && g->manager_registered)
        (void)source_send(g, AURORA_SOURCE_ABSENT, 0, 0, 0);
    fail_closed(g);
}

static void close_upstream_sources(struct gate *g)
{
    close_fd(&g->source_fd);
    close_fd(&g->local_fd);
    mark_source_absent(g);
}

static int forward_bridge_frame(struct gate *g, uint8_t *frame, size_t len)
{
    uint16_t kind;
    uint32_t flags;
    uint8_t *payload;
    uint32_t payload_len;
    if (validate_aurora_frame(frame, len, &kind, &flags, &payload, &payload_len) < 0)
        return -1;
    (void)flags;
    (void)payload;
    (void)payload_len;

    if (kind == AURORA_USB_ENCODED_IEC61937) {
        if (g->source_fd >= 0) {
            mark_source_present(g);
            if (send_packet(g->source_fd, frame, len) < 0) {
                close_fd(&g->source_fd);
                mark_source_absent(g);
            }
        }
        return 0;
    }

    /* CONFIG ACK/ERROR, CLOCK_REPORT and other control traffic is safe to
     * duplicate to source adapters. Each adapter keeps its own configured and
     * drift state; IEC61937 media itself is never duplicated to local music. */
    if (g->source_fd >= 0 && send_packet(g->source_fd, frame, len) < 0) {
        close_fd(&g->source_fd);
        mark_source_absent(g);
    }
    if (g->local_fd >= 0 && send_packet(g->local_fd, frame, len) < 0)
        close_fd(&g->local_fd);
    return 0;
}

static int local_frame_allowed(uint16_t kind)
{
    return kind == AURORA_USB_CONFIG ||
           kind == AURORA_USB_PCM_S32LE ||
           kind == AURORA_USB_PING;
}

int main(void)
{
    signal(SIGINT, on_signal);
    signal(SIGTERM, on_signal);
    signal(SIGPIPE, SIG_IGN);

    struct gate g;
    memset(&g, 0, sizeof(g));
    g.bridge_fd = -1;
    g.source_listen_fd = -1;
    g.source_fd = -1;
    g.local_listen_fd = -1;
    g.local_fd = -1;
    g.manager_fd = -1;
    g.user_gain = 1.0f;

    const char *real_bridge = env_or("AURORA_USB_BRIDGE_SOCKET_REAL", DEFAULT_REAL_BRIDGE);
    const char *source_socket = env_or("AURORA_HDMI_SOURCE_SOCKET", DEFAULT_SOURCE_SOCKET);
    const char *local_socket = env_or("AURORA_LOCAL_SOURCE_SOCKET", DEFAULT_LOCAL_SOURCE_SOCKET);
    const char *manager_socket = env_or("AURORA_SOURCE_MANAGER_SOCKET", DEFAULT_MANAGER_SOCKET);

    g.source_listen_fd = open_listener(source_socket);
    if (g.source_listen_fd < 0) {
        perror("aurora-source-gate: HDMI source listener");
        return 1;
    }
    g.local_listen_fd = open_listener(local_socket);
    if (g.local_listen_fd < 0) {
        perror("aurora-source-gate: local source listener");
        close(g.source_listen_fd);
        unlink(source_socket);
        return 1;
    }

    uint8_t *source_buf = malloc(AURORA_USB_MAX_FRAME);
    uint8_t *bridge_buf = malloc(AURORA_USB_MAX_FRAME);
    if (!source_buf || !bridge_buf) {
        perror("aurora-source-gate: buffers");
        free(source_buf);
        free(bridge_buf);
        close(g.source_listen_fd);
        close(g.local_listen_fd);
        unlink(source_socket);
        unlink(local_socket);
        return 1;
    }

    while (!stop_requested) {
        if (g.bridge_fd < 0)
            g.bridge_fd = connect_seqpacket(real_bridge);
        if (g.manager_fd < 0) {
            g.manager_fd = connect_seqpacket(manager_socket);
            if (g.manager_fd >= 0) {
                g.manager_registered = 0;
                g.source_sequence = 0;
                if (register_with_manager(&g) < 0) {
                    close_fd(&g.manager_fd);
                    fail_closed(&g);
                }
            }
        }

        struct pollfd p[6];
        p[0] = (struct pollfd){.fd = g.source_listen_fd, .events = POLLIN};
        p[1] = (struct pollfd){.fd = g.source_fd, .events = POLLIN};
        p[2] = (struct pollfd){.fd = g.bridge_fd, .events = POLLIN};
        p[3] = (struct pollfd){.fd = g.manager_fd, .events = POLLIN};
        p[4] = (struct pollfd){.fd = g.local_listen_fd, .events = POLLIN};
        p[5] = (struct pollfd){.fd = g.local_fd, .events = POLLIN};
        int rc = poll(p, 6, POLL_TIMEOUT_MS);
        if (rc < 0) {
            if (errno == EINTR)
                continue;
            break;
        }

        if (p[0].revents & POLLIN) {
            int accepted = accept4(g.source_listen_fd, NULL, NULL, SOCK_CLOEXEC);
            if (accepted >= 0) {
                if (g.source_fd >= 0)
                    close(accepted);
                else
                    g.source_fd = accepted;
            }
        }
        if (p[4].revents & POLLIN) {
            int accepted = accept4(g.local_listen_fd, NULL, NULL, SOCK_CLOEXEC);
            if (accepted >= 0) {
                if (g.local_fd >= 0)
                    close(accepted);
                else
                    g.local_fd = accepted;
            }
        }

        if (g.manager_fd >= 0 &&
            (p[3].revents & (POLLHUP | POLLERR | POLLNVAL))) {
            close_fd(&g.manager_fd);
            g.manager_registered = 0;
            fail_closed(&g);
        } else if (g.manager_fd >= 0 && (p[3].revents & POLLIN)) {
            uint8_t message[AURORA_SOURCE_MESSAGE_BYTES];
            ssize_t n = recv(g.manager_fd, message, sizeof(message), 0);
            if (n <= 0 || handle_manager_message(&g, message, (size_t)n) < 0) {
                close_fd(&g.manager_fd);
                g.manager_registered = 0;
                fail_closed(&g);
            }
        }

        if (g.bridge_fd >= 0 &&
            (p[2].revents & (POLLHUP | POLLERR | POLLNVAL))) {
            close_fd(&g.bridge_fd);
            close_upstream_sources(&g);
        } else if (g.bridge_fd >= 0 && (p[2].revents & POLLIN)) {
            ssize_t n = recv(g.bridge_fd, bridge_buf, AURORA_USB_MAX_FRAME, 0);
            if (n <= 0) {
                close_fd(&g.bridge_fd);
                close_upstream_sources(&g);
            } else if (forward_bridge_frame(&g, bridge_buf, (size_t)n) < 0) {
                close_fd(&g.bridge_fd);
                close_upstream_sources(&g);
            }
        }

        if (g.source_fd >= 0 &&
            (p[1].revents & (POLLHUP | POLLERR | POLLNVAL))) {
            close_fd(&g.source_fd);
            mark_source_absent(&g);
        } else if (g.source_fd >= 0 && g.bridge_fd >= 0 && (p[1].revents & POLLIN)) {
            ssize_t n = recv(g.source_fd, source_buf, AURORA_USB_MAX_FRAME, 0);
            if (n <= 0) {
                close_fd(&g.source_fd);
                mark_source_absent(&g);
            } else {
                int gate_rc = apply_pcm_gate(&g, source_buf, (size_t)n);
                if (gate_rc < 0) {
                    close_fd(&g.source_fd);
                    mark_source_absent(&g);
                } else if (gate_rc == 0 &&
                           send_packet(g.bridge_fd, source_buf, (size_t)n) < 0) {
                    close_fd(&g.bridge_fd);
                    close_upstream_sources(&g);
                }
            }
        }

        if (g.local_fd >= 0 &&
            (p[5].revents & (POLLHUP | POLLERR | POLLNVAL))) {
            close_fd(&g.local_fd);
        } else if (g.local_fd >= 0 && g.bridge_fd >= 0 && (p[5].revents & POLLIN)) {
            ssize_t n = recv(g.local_fd, source_buf, AURORA_USB_MAX_FRAME, 0);
            uint16_t kind;
            uint32_t flags, payload_len;
            uint8_t *payload;
            if (n <= 0) {
                close_fd(&g.local_fd);
            } else if (validate_aurora_frame(source_buf, (size_t)n, &kind, &flags,
                                             &payload, &payload_len) < 0 ||
                       !local_frame_allowed(kind)) {
                (void)flags;
                (void)payload;
                (void)payload_len;
                close_fd(&g.local_fd);
            } else if (send_packet(g.bridge_fd, source_buf, (size_t)n) < 0) {
                close_fd(&g.bridge_fd);
                close_upstream_sources(&g);
            }
        }

        uint64_t now = monotonic_ms();
        if (g.source_present && g.last_encoded_ms != 0 &&
            now > g.last_encoded_ms + SOURCE_IDLE_MS)
            mark_source_absent(&g);

        if (g.pending_quiesce && g.quiesce_deadline_ms != 0 &&
            now >= g.quiesce_deadline_ms) {
            if (complete_quiesce(&g) < 0) {
                close_fd(&g.manager_fd);
                g.manager_registered = 0;
                fail_closed(&g);
            }
        }
    }

    free(source_buf);
    free(bridge_buf);
    close_fd(&g.source_fd);
    close_fd(&g.local_fd);
    close_fd(&g.bridge_fd);
    close_fd(&g.manager_fd);
    close_fd(&g.source_listen_fd);
    close_fd(&g.local_listen_fd);
    unlink(source_socket);
    unlink(local_socket);
    return 0;
}