#define _GNU_SOURCE

#include <errno.h>
#include <fcntl.h>
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
#include <sys/wait.h>
#include <unistd.h>

#include "aurora_usb_v1.h"

#define BRIDGE_SOCKET "/run/aurora/usb-bridge.sock"
#define DEFAULT_ORENDER "/opt/aurora/external/orender"
#define DEFAULT_HARLETTY "/opt/aurora/external/libharletty_bridge.so"
#define DEFAULT_LAYOUT "/etc/aurora/layouts/7.1.4.yaml"
#define DEFAULT_SURROUND_UPMIX "/usr/local/sbin/aurora-surround-upmix"
#define DEFAULT_POSTPROCESS "/usr/local/bin/aurora-s6-postprocess"

#define PCM_CHANNELS AURORA_USB_CHANNELS_7_1_4
#define PCM_PERIOD_FRAMES AURORA_USB_PERIOD_FRAMES
#define PCM_PERIOD_SAMPLES (PCM_CHANNELS * PCM_PERIOD_FRAMES)
#define PCM_PERIOD_BYTES (PCM_PERIOD_SAMPLES * 4u)
#define TX_FRAME_CAP (AURORA_USB_HEADER_LEN + PCM_PERIOD_BYTES)
#define RX_FRAME_CAP AURORA_USB_MAX_FRAME
#define RENDER_READ_CHUNK (64u * 1024u)
#define RENDER_PENDING_CAP (128u * 1024u)
#define IO_TIMEOUT_MS 100
#define ENCODED_PENDING_CAP (2u * AURORA_USB_MAX_FRAME)

#define POST_CONTROL_MAGIC_U32 0x30435041u /* "APC0" */
#define POST_CONTROL_VERSION 1u
#define POST_CONTROL_BYTES 32u
#define POST_CTRL_RESET 1u
#define POST_CTRL_CLOCK_REPORT 2u
#define POST_CTRL_LIPSYNC_FRAMES 3u
#define POST_CTRL_MASTER_GAIN_MDB 4u
#define POST_CTRL_MUTE 5u
#define POST_CTRL_STANDBY 6u

static volatile sig_atomic_t stop_requested;

/* SHA-256 of the protocol-v1 canonical output-layout manifest:
 * AURORA_LAYOUT_V1;id=1;rate=48000;format=S32LE;period=40;channels=FL,FR,C,LFE,BL,BR,SL,SR,TFL,TFR,TBL,TBR\n
 */
static const uint8_t layout_hash_7_1_4_v1[32] = {
    0x05, 0x06, 0x35, 0x60, 0xd6, 0xc5, 0xc1, 0xb7,
    0xd3, 0x70, 0x96, 0x56, 0xcd, 0x8c, 0x64, 0x4a,
    0x6d, 0x2b, 0x52, 0xf5, 0xe8, 0x13, 0x83, 0x77,
    0x1f, 0x26, 0x32, 0x34, 0x42, 0xd0, 0xa2, 0x44,
};

struct renderer_proc {
    pid_t pid;
    pid_t post_pid;
    int stdin_fd;
    int stdout_fd;
    int control_fd;
    int postprocess_enabled;
};

struct app {
    int bridge_fd;
    uint32_t tx_sequence;
    int configured;
    uint64_t next_pcm_pts;
    int have_pcm_pts;
    uint32_t next_pcm_flags;
    struct renderer_proc renderer;
    uint64_t renderer_serial;
    uint8_t render_pending[RENDER_PENDING_CAP];
    size_t render_pending_len;
    uint8_t encoded_pending[ENCODED_PENDING_CAP];
    size_t encoded_pending_offset;
    size_t encoded_pending_len;
    uint64_t encoded_frames;
    uint64_t encoded_bytes;
    uint64_t pcm_periods;
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

static const char *env_or(const char *key, const char *fallback)
{
    const char *v = getenv(key);
    return (v && *v) ? v : fallback;
}

static int postprocess_disabled(const char *path)
{
    return strcmp(path, "disabled") == 0 || strcmp(path, "none") == 0;
}

static int wait_fd(int fd, short events)
{
    struct pollfd p = {.fd = fd, .events = events};
    for (;;) {
        int rc = poll(&p, 1, IO_TIMEOUT_MS);
        if (rc > 0)
            return (p.revents & events) ? 0 : -1;
        if (rc == 0) {
            errno = ETIMEDOUT;
            return -1;
        }
        if (errno != EINTR)
            return -1;
    }
}

static int write_all_timeout(int fd, const uint8_t *buf, size_t len)
{
    while (len) {
        ssize_t n = write(fd, buf, len);
        if (n > 0) {
            buf += (size_t)n;
            len -= (size_t)n;
            continue;
        }
        if (n < 0 && errno == EINTR)
            continue;
        if (n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK) &&
            wait_fd(fd, POLLOUT) == 0)
            continue;
        return -1;
    }
    return 0;
}

/* This broker is a process I/O loop, not an audio-device callback. Never
 * wait for decoder input while its output may need draining on this thread. */
static int flush_encoded(struct app *a)
{
    if (!a->encoded_pending_len)
        return 0;
    ssize_t n = write(a->renderer.stdin_fd,
                      a->encoded_pending + a->encoded_pending_offset,
                      a->encoded_pending_len);
    if (n > 0) {
        a->encoded_pending_offset += (size_t)n;
        a->encoded_pending_len -= (size_t)n;
        if (!a->encoded_pending_len)
            a->encoded_pending_offset = 0;
        return 0;
    }
    if (n < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK))
        return 0;
    return -1;
}

static int queue_encoded(struct app *a, const uint8_t *payload, size_t len)
{
    if (len > sizeof(a->encoded_pending) - a->encoded_pending_len) {
        errno = ENOBUFS;
        return -1;
    }
    if (len > sizeof(a->encoded_pending) - a->encoded_pending_offset -
              a->encoded_pending_len) {
        memmove(a->encoded_pending,
                a->encoded_pending + a->encoded_pending_offset,
                a->encoded_pending_len);
        a->encoded_pending_offset = 0;
    }
    memcpy(a->encoded_pending + a->encoded_pending_offset +
           a->encoded_pending_len, payload, len);
    a->encoded_pending_len += len;
    return flush_encoded(a);
}

static void close_if_valid(int *fd)
{
    if (*fd >= 0) {
        close(*fd);
        *fd = -1;
    }
}

static void renderer_close_fds(struct renderer_proc *r)
{
    close_if_valid(&r->stdin_fd);
    close_if_valid(&r->stdout_fd);
    close_if_valid(&r->control_fd);
}

static void terminate_child(pid_t *pid)
{
    if (*pid <= 0)
        return;

    kill(*pid, SIGTERM);
    for (int i = 0; i < 20; ++i) {
        pid_t w = waitpid(*pid, NULL, WNOHANG);
        if (w == *pid) {
            *pid = -1;
            return;
        }
        usleep(10000);
    }
    kill(*pid, SIGKILL);
    (void)waitpid(*pid, NULL, 0);
    *pid = -1;
}

static void renderer_stop(struct renderer_proc *r)
{
    renderer_close_fds(r);
    terminate_child(&r->pid);
    terminate_child(&r->post_pid);
    r->postprocess_enabled = 0;
}

static void close_child_fds_above(int keep0, int keep1, int keep3,
                                  const int *fds, size_t count)
{
    for (size_t i = 0; i < count; ++i) {
        int fd = fds[i];
        if (fd >= 0 && fd != keep0 && fd != keep1 && fd != keep3 && fd > 2)
            close(fd);
    }
}

static int renderer_start(struct renderer_proc *r)
{
    const char *mode = env_or("AURORA_DECODE_MODE", "objects");
    int surround_upmix = strcmp(mode, "surround-upmix") == 0;
    if (!surround_upmix && strcmp(mode, "objects") != 0) {
        fprintf(stderr, "aurora-live-ingest: invalid AURORA_DECODE_MODE=%s\n", mode);
        errno = EINVAL;
        return -1;
    }
    const char *fallback = env_or("AURORA_SURROUND_UPMIX_BIN", DEFAULT_SURROUND_UPMIX);
    const char *orender = env_or("AURORA_ORENDER_BIN", DEFAULT_ORENDER);
    const char *bridge = env_or("AURORA_HARLETTY_BRIDGE", DEFAULT_HARLETTY);
    const char *layout = env_or("AURORA_7_1_4_LAYOUT", DEFAULT_LAYOUT);
    const char *postprocess = env_or("AURORA_POSTPROCESS_BIN", DEFAULT_POSTPROCESS);
    int enable_post = !postprocess_disabled(postprocess);
    int to_child[2] = {-1, -1};
    int from_child[2] = {-1, -1};
    int render_to_post[2] = {-1, -1};
    int control_to_post[2] = {-1, -1};

    if (pipe2(to_child, O_CLOEXEC) < 0)
        return -1;
    if (pipe2(from_child, O_CLOEXEC) < 0)
        goto fail;
    if (enable_post && pipe2(render_to_post, O_CLOEXEC) < 0)
        goto fail;
    if (enable_post && pipe2(control_to_post, O_CLOEXEC) < 0)
        goto fail;

    pid_t pid = fork();
    if (pid < 0)
        goto fail;

    if (pid == 0) {
        int output_fd = enable_post ? render_to_post[1] : from_child[1];
        if (dup2(to_child[0], STDIN_FILENO) < 0 ||
            dup2(output_fd, STDOUT_FILENO) < 0)
            _exit(126);

        int all_fds[] = {
            to_child[0], to_child[1], from_child[0], from_child[1],
            render_to_post[0], render_to_post[1],
            control_to_post[0], control_to_post[1]
        };
        close_child_fds_above(STDIN_FILENO, STDOUT_FILENO, -1,
                              all_fds, sizeof(all_fds) / sizeof(all_fds[0]));

        if (surround_upmix) {
            execl(fallback, fallback, (char *)NULL);
            perror("aurora-live-ingest: exec surround upmixer");
            _exit(127);
        }

        /* Omniphony's stdin decoder thread detects IEC61937 sync, maintains a
         * streaming SpdifParser across arbitrary read boundaries, and passes
         * unwrapped packets + their IEC61937 data_type to Harletty. */
        execl(orender, orender,
              "-",
              "--bridge-path", bridge,
              "--enable-vbap",
              "--speaker-layout", layout,
              "--continuous",
              "--no-osc",
              "--output-backend", "file",
              "--output-file", "-",
              "--output-file-format", "raw-f32",
              (char *)NULL);
        perror("aurora-live-ingest: exec orender");
        _exit(127);
    }

    pid_t post_pid = -1;
    if (enable_post) {
        post_pid = fork();
        if (post_pid < 0) {
            kill(pid, SIGTERM);
            (void)waitpid(pid, NULL, 0);
            goto fail;
        }
        if (post_pid == 0) {
            if (dup2(render_to_post[0], STDIN_FILENO) < 0 ||
                dup2(from_child[1], STDOUT_FILENO) < 0 ||
                dup2(control_to_post[0], 3) < 0)
                _exit(126);

            int all_fds[] = {
                to_child[0], to_child[1], from_child[0], from_child[1],
                render_to_post[0], render_to_post[1],
                control_to_post[0], control_to_post[1]
            };
            close_child_fds_above(STDIN_FILENO, STDOUT_FILENO, 3,
                                  all_fds, sizeof(all_fds) / sizeof(all_fds[0]));
            setenv("AURORA_CONTROL_FD", "3", 1);
            execl(postprocess, postprocess, (char *)NULL);
            perror("aurora-live-ingest: exec postprocessor");
            _exit(127);
        }
    }

    close(to_child[0]);
    close(from_child[1]);
    if (enable_post) {
        close(render_to_post[0]);
        close(render_to_post[1]);
        close(control_to_post[0]);
    }

    int flags = fcntl(to_child[1], F_GETFL, 0);
    if (flags >= 0)
        (void)fcntl(to_child[1], F_SETFL, flags | O_NONBLOCK);
    flags = fcntl(from_child[0], F_GETFL, 0);
    if (flags >= 0)
        (void)fcntl(from_child[0], F_SETFL, flags | O_NONBLOCK);
    if (enable_post) {
        flags = fcntl(control_to_post[1], F_GETFL, 0);
        if (flags >= 0)
            (void)fcntl(control_to_post[1], F_SETFL, flags | O_NONBLOCK);
    }

    r->pid = pid;
    r->post_pid = post_pid;
    r->stdin_fd = to_child[1];
    r->stdout_fd = from_child[0];
    r->control_fd = enable_post ? control_to_post[1] : -1;
    r->postprocess_enabled = enable_post;
    fprintf(stderr,
            "aurora-live-ingest: decode_mode=%s pid=%ld postprocess=%s pid=%ld\n",
            mode, (long)pid, enable_post ? "enabled" : "disabled", (long)post_pid);
    return 0;

fail:
    if (to_child[0] >= 0) close(to_child[0]);
    if (to_child[1] >= 0) close(to_child[1]);
    if (from_child[0] >= 0) close(from_child[0]);
    if (from_child[1] >= 0) close(from_child[1]);
    if (render_to_post[0] >= 0) close(render_to_post[0]);
    if (render_to_post[1] >= 0) close(render_to_post[1]);
    if (control_to_post[0] >= 0) close(control_to_post[0]);
    if (control_to_post[1] >= 0) close(control_to_post[1]);
    return -1;
}

static int renderer_send_control(struct renderer_proc *r, uint16_t kind,
                                 uint64_t data0, uint64_t data1,
                                 uint32_t data2, uint32_t flags)
{
    if (!r->postprocess_enabled || r->control_fd < 0)
        return 0;

    uint8_t message[POST_CONTROL_BYTES] = {0};
    write_le32(message + 0, POST_CONTROL_MAGIC_U32);
    write_le16(message + 4, POST_CONTROL_VERSION);
    write_le16(message + 6, kind);
    write_le64(message + 8, data0);
    write_le64(message + 16, data1);
    write_le32(message + 24, data2);
    write_le32(message + 28, flags);
    return write_all_timeout(r->control_fd, message, sizeof(message));
}

static int connect_bridge(void)
{
    const char *bridge_socket = env_or("AURORA_USB_BRIDGE_SOCKET", BRIDGE_SOCKET);
    int fd = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return -1;

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    if (strlen(bridge_socket) >= sizeof(addr.sun_path)) {
        close(fd);
        errno = ENAMETOOLONG;
        return -1;
    }
    strcpy(addr.sun_path, bridge_socket);

    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        int saved = errno;
        close(fd);
        errno = saved;
        return -1;
    }
    return fd;
}

static int send_frame(struct app *a, uint16_t kind, uint32_t flags,
                      uint64_t pts_48k, uint32_t aux,
                      const uint8_t *payload, uint32_t payload_len)
{
    uint8_t frame[TX_FRAME_CAP];
    size_t total = AURORA_USB_HEADER_LEN + (size_t)payload_len;
    if (total > sizeof(frame) || (payload_len && !payload))
        return -1;

    memset(frame, 0, AURORA_USB_HEADER_LEN);
    write_le32(frame + 0, AURORA_USB_MAGIC_U32);
    write_le16(frame + 4, AURORA_USB_VERSION);
    write_le16(frame + 6, kind);
    write_le32(frame + 8, flags);
    write_le32(frame + 12, a->tx_sequence);
    write_le64(frame + 16, pts_48k);
    write_le32(frame + 24, payload_len);
    write_le32(frame + 28, aux);
    if (payload_len)
        memcpy(frame + AURORA_USB_HEADER_LEN, payload, payload_len);

    ssize_t n = send(a->bridge_fd, frame, total, MSG_NOSIGNAL | MSG_DONTWAIT);
    if (n == (ssize_t)total) {
        a->tx_sequence++;
        return 0;
    }
    if (n < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK))
        return 1; /* Retain the complete packet and retry on POLLOUT. */
    return -1;
}

static int send_config(struct app *a)
{
    uint8_t payload[48] = {0};
    write_le32(payload + 0, AURORA_USB_SAMPLE_RATE_HZ);
    write_le16(payload + 4, AURORA_USB_PERIOD_FRAMES);
    write_le16(payload + 6, AURORA_USB_CHANNELS_7_1_4);
    write_le16(payload + 8, AURORA_USB_PCM_FORMAT_S32LE);
    write_le16(payload + 10, AURORA_USB_LAYOUT_ID_7_1_4);
    memcpy(payload + 16, layout_hash_7_1_4_v1, sizeof(layout_hash_7_1_4_v1));

    a->configured = 0;
    return send_frame(a, AURORA_USB_CONFIG, 0, 0, 0, payload, sizeof(payload));
}

static int32_t f32_to_s32(float x)
{
    if (!isfinite(x))
        return 0;
    if (x >= 1.0f)
        return INT32_MAX;
    if (x <= -1.0f)
        return INT32_MIN;
    return (int32_t)llround((double)x * 2147483647.0);
}

static int flush_pcm_periods(struct app *a)
{
    while (a->render_pending_len >= PCM_PERIOD_BYTES) {
        /* CONFIG is the fail-closed gate. Preserve already-rendered audio
         * while waiting for the ACK instead of discarding the first period. */
        if (!a->configured)
            break;

        uint8_t out[PCM_PERIOD_BYTES];
        for (size_t i = 0; i < PCM_PERIOD_SAMPLES; ++i) {
            float f;
            memcpy(&f, a->render_pending + i * sizeof(float), sizeof(float));
            write_le32(out + i * 4u, (uint32_t)f32_to_s32(f));
        }

        uint32_t aux = AURORA_USB_PCM_AUX(PCM_CHANNELS, PCM_PERIOD_FRAMES);
        uint32_t flags = a->next_pcm_flags;
        if (a->have_pcm_pts)
            flags |= AURORA_USB_FLAG_PTS_VALID;
        int rc = send_frame(a, AURORA_USB_PCM_S32LE, flags,
                            a->next_pcm_pts, aux, out, sizeof(out));
        if (rc < 0)
            return -1;
        if (rc > 0)
            break;

        a->next_pcm_flags = 0;
        if (a->have_pcm_pts)
            a->next_pcm_pts += PCM_PERIOD_FRAMES;
        a->pcm_periods++;

        memmove(a->render_pending,
                a->render_pending + PCM_PERIOD_BYTES,
                a->render_pending_len - PCM_PERIOD_BYTES);
        a->render_pending_len -= PCM_PERIOD_BYTES;
    }
    return 0;
}

static int ingest_renderer_output(struct app *a)
{
    uint8_t tmp[RENDER_READ_CHUNK];
    size_t available = sizeof(a->render_pending) - a->render_pending_len;
    if (!available)
        return 0;
    size_t count = available < sizeof(tmp) ? available : sizeof(tmp);
    /* One bounded read per poll cycle keeps input/control events responsive. */
    ssize_t n = read(a->renderer.stdout_fd, tmp, count);
    if (n > 0) {
        memcpy(a->render_pending + a->render_pending_len, tmp, (size_t)n);
        a->render_pending_len += (size_t)n;
        return flush_pcm_periods(a);
    }
    if (n == 0)
        return 1;
    if (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK)
        return 0;
    return -1;
}

static int validate_frame(const uint8_t *frame, size_t len,
                          uint16_t *kind, uint32_t *flags,
                          uint64_t *pts, const uint8_t **payload,
                          uint32_t *payload_len)
{
    if (len > RX_FRAME_CAP || len < AURORA_USB_HEADER_LEN ||
        read_le32(frame + 0) != AURORA_USB_MAGIC_U32 ||
        read_le16(frame + 4) != AURORA_USB_VERSION)
        return -1;

    uint32_t plen = read_le32(frame + 24);
    if ((size_t)plen + AURORA_USB_HEADER_LEN != len)
        return -1;

    if (kind) *kind = read_le16(frame + 6);
    if (flags) *flags = read_le32(frame + 8);
    if (pts) *pts = read_le64(frame + 16);
    if (payload) *payload = frame + AURORA_USB_HEADER_LEN;
    if (payload_len) *payload_len = plen;
    return 0;
}

static int restart_renderer(struct app *a, uint64_t pts, int pts_valid)
{
    a->renderer_serial++;
    a->encoded_pending_offset = 0;
    a->encoded_pending_len = 0;
    a->render_pending_len = 0;
    a->next_pcm_pts = pts;
    a->have_pcm_pts = pts_valid;
    renderer_stop(&a->renderer);
    return renderer_start(&a->renderer);
}

static int handle_clock_report(struct app *a, const uint8_t *payload,
                               uint32_t payload_len, uint32_t frame_flags)
{
    if (payload_len != sizeof(struct aurora_usb_clock_report_v1))
        return -1;

    uint64_t sink = read_le64(payload + 0);
    uint64_t source = read_le64(payload + 8);
    uint32_t queued = read_le32(payload + 16);
    uint32_t capture_flags = read_le32(payload + 20);
    uint32_t combined_flags = capture_flags | frame_flags;

    if ((combined_flags & AURORA_USB_FLAG_XRUN_RECOVERY) != 0) {
        a->next_pcm_flags |= AURORA_USB_FLAG_XRUN_RECOVERY |
                             AURORA_USB_FLAG_DISCONTINUITY;
        if (renderer_send_control(&a->renderer, POST_CTRL_RESET,
                                  0, 0, 0, combined_flags) < 0)
            return -1;
    }

    return renderer_send_control(&a->renderer, POST_CTRL_CLOCK_REPORT,
                                 sink, source, queued, combined_flags);
}

static int handle_bridge_frame(struct app *a, const uint8_t *frame, size_t len)
{
    uint16_t kind;
    uint32_t flags;
    uint64_t pts;
    const uint8_t *payload;
    uint32_t payload_len;

    if (validate_frame(frame, len, &kind, &flags, &pts,
                       &payload, &payload_len) < 0)
        return -1;

    switch (kind) {
    case AURORA_USB_ENCODED_IEC61937:
        if ((flags & AURORA_USB_FLAG_DISCONTINUITY) != 0) {
            a->next_pcm_flags |= AURORA_USB_FLAG_DISCONTINUITY;
            if (restart_renderer(a, pts,
                                 (flags & AURORA_USB_FLAG_PTS_VALID) != 0) < 0)
                return -1;
        }

        if (!a->have_pcm_pts && (flags & AURORA_USB_FLAG_PTS_VALID) != 0) {
            a->have_pcm_pts = 1;
            a->next_pcm_pts = pts;
        }

        /* Do NOT strip IEC61937 here. Omniphony v0.5.2 maintains a streaming
         * SpdifParser on stdin and forwards packet.payload + data_type to the
         * Harletty bridge as RInputTransport::Iec61937. */
        if (queue_encoded(a, payload, payload_len) < 0)
            return -1;
        a->encoded_frames++;
        a->encoded_bytes += payload_len;
        break;

    case AURORA_USB_ACK:
        if (payload_len == 4 && read_le16(payload) == AURORA_USB_CONFIG &&
            read_le16(payload + 2) == 0) {
            a->configured = 1;
            fprintf(stderr, "aurora-live-ingest: STM32 CONFIG accepted\n");
            if (flush_pcm_periods(a) < 0)
                return -1;
        }
        break;

    case AURORA_USB_ERROR:
        if (payload_len >= 4) {
            uint16_t offending = read_le16(payload);
            uint16_t code = read_le16(payload + 2);
            fprintf(stderr,
                    "aurora-live-ingest: STM32 error offending=%u code=%u\n",
                    offending, code);
            if (offending == AURORA_USB_CONFIG) {
                a->configured = 0;
                a->render_pending_len = 0;
            }
            if (offending == AURORA_USB_PCM_S32LE) {
                a->next_pcm_flags |= AURORA_USB_FLAG_XRUN_RECOVERY |
                                     AURORA_USB_FLAG_DISCONTINUITY;
                (void)renderer_send_control(&a->renderer, POST_CTRL_RESET,
                                            0, 0, 0,
                                            AURORA_USB_FLAG_XRUN_RECOVERY);
            }
        }
        break;

    case AURORA_USB_CLOCK_REPORT:
        return handle_clock_report(a, payload, payload_len, flags);

    case AURORA_USB_PONG:
        break;

    default:
        break;
    }

    return 0;
}

static int reset_after_bridge_disconnect(struct app *a)
{
    a->configured = 0;
    a->render_pending_len = 0;
    a->have_pcm_pts = 0;
    a->next_pcm_pts = 0;
    a->next_pcm_flags = AURORA_USB_FLAG_DISCONTINUITY |
                        AURORA_USB_FLAG_XRUN_RECOVERY;
    return restart_renderer(a, 0, 0);
}

int main(void)
{
    signal(SIGINT, on_signal);
    signal(SIGTERM, on_signal);
    signal(SIGPIPE, SIG_IGN);

    struct app a;
    memset(&a, 0, sizeof(a));
    a.bridge_fd = -1;
    a.renderer.pid = -1;
    a.renderer.post_pid = -1;
    a.renderer.stdin_fd = -1;
    a.renderer.stdout_fd = -1;
    a.renderer.control_fd = -1;

    if (renderer_start(&a.renderer) < 0) {
        perror("aurora-live-ingest: start renderer pipeline");
        return 1;
    }

    uint8_t *rx = malloc(RX_FRAME_CAP);
    if (!rx) {
        perror("aurora-live-ingest: rx buffer");
        renderer_stop(&a.renderer);
        return 1;
    }

    while (!stop_requested) {
        if (a.bridge_fd < 0) {
            a.bridge_fd = connect_bridge();
            if (a.bridge_fd < 0) {
                usleep(100000);
                continue;
            }
            a.tx_sequence = 0;
            a.configured = 0;
            a.render_pending_len = 0;
            if (send_config(&a) != 0) {
                close(a.bridge_fd);
                a.bridge_fd = -1;
                continue;
            }
            fprintf(stderr, "aurora-live-ingest: connected to %s\n",
                    env_or("AURORA_USB_BRIDGE_SOCKET", BRIDGE_SOCKET));
        }

        struct pollfd p[3];
        short bridge_events = POLLIN;
        if (a.configured && a.render_pending_len >= PCM_PERIOD_BYTES)
            bridge_events |= POLLOUT;
        p[0] = (struct pollfd){.fd = a.bridge_fd, .events = bridge_events};
        /* A full output queue applies pipe backpressure without discarding PCM. */
        p[1] = (struct pollfd){
            .fd = a.render_pending_len < RENDER_PENDING_CAP ? a.renderer.stdout_fd : -1,
            .events = POLLIN
        };
        p[2] = (struct pollfd){
            .fd = a.encoded_pending_len ? a.renderer.stdin_fd : -1,
            .events = POLLOUT
        };
        uint64_t renderer_serial_at_poll = a.renderer_serial;

        int rc = poll(p, 3, 250);
        if (rc < 0) {
            if (errno == EINTR)
                continue;
            perror("aurora-live-ingest: poll");
            break;
        }

        if (p[0].revents & (POLLHUP | POLLERR | POLLNVAL)) {
            close(a.bridge_fd);
            a.bridge_fd = -1;
            if (reset_after_bridge_disconnect(&a) < 0)
                break;
            continue;
        }

        if (p[0].revents & POLLIN) {
            ssize_t n = recv(a.bridge_fd, rx, RX_FRAME_CAP, MSG_TRUNC);
            if (n <= 0) {
                close(a.bridge_fd);
                a.bridge_fd = -1;
                if (reset_after_bridge_disconnect(&a) < 0)
                    break;
                continue;
            }
            if (handle_bridge_frame(&a, rx, (size_t)n) < 0) {
                fprintf(stderr, "aurora-live-ingest: input/control failure; restarting renderer pipeline\n");
                a.next_pcm_flags |= AURORA_USB_FLAG_DISCONTINUITY |
                                    AURORA_USB_FLAG_XRUN_RECOVERY;
                if (restart_renderer(&a, a.next_pcm_pts, a.have_pcm_pts) < 0)
                    break;
            }
        }

        /* If the bridge-side event restarted the pipeline, revents for the old
         * descriptor are stale and must not be applied to the new renderer. */
        if (renderer_serial_at_poll != a.renderer_serial)
            continue;

        if ((p[0].revents & POLLOUT) && flush_pcm_periods(&a) < 0) {
            close(a.bridge_fd);
            a.bridge_fd = -1;
            if (reset_after_bridge_disconnect(&a) < 0)
                break;
            continue;
        }

        if (p[2].revents & (POLLOUT | POLLHUP | POLLERR | POLLNVAL)) {
            if (flush_encoded(&a) < 0) {
                a.next_pcm_flags |= AURORA_USB_FLAG_DISCONTINUITY |
                                    AURORA_USB_FLAG_XRUN_RECOVERY;
                if (restart_renderer(&a, a.next_pcm_pts, a.have_pcm_pts) < 0)
                    break;
                continue;
            }
        }

        if (p[1].revents & POLLIN) {
            int rr = ingest_renderer_output(&a);
            if (rr < 0) {
                fprintf(stderr, "aurora-live-ingest: renderer/postprocessor output failure\n");
                a.next_pcm_flags |= AURORA_USB_FLAG_DISCONTINUITY |
                                    AURORA_USB_FLAG_XRUN_RECOVERY;
                if (restart_renderer(&a, a.next_pcm_pts, a.have_pcm_pts) < 0)
                    break;
            } else if (rr > 0) {
                a.next_pcm_flags |= AURORA_USB_FLAG_DISCONTINUITY;
                if (restart_renderer(&a, a.next_pcm_pts, a.have_pcm_pts) < 0)
                    break;
            }
        }

        if (renderer_serial_at_poll != a.renderer_serial)
            continue;

        if (!(p[1].revents & POLLIN) &&
            (p[1].revents & (POLLHUP | POLLERR | POLLNVAL))) {
            a.next_pcm_flags |= AURORA_USB_FLAG_DISCONTINUITY |
                                AURORA_USB_FLAG_XRUN_RECOVERY;
            if (restart_renderer(&a, a.next_pcm_pts, a.have_pcm_pts) < 0)
                break;
        }
    }

    fprintf(stderr,
            "aurora-live-ingest: stop encoded_frames=%llu encoded_bytes=%llu pcm_periods=%llu\n",
            (unsigned long long)a.encoded_frames,
            (unsigned long long)a.encoded_bytes,
            (unsigned long long)a.pcm_periods);

    free(rx);
    if (a.bridge_fd >= 0)
        close(a.bridge_fd);
    renderer_stop(&a.renderer);
    return 0;
}

