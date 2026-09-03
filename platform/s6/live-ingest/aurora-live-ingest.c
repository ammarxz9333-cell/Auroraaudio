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

#define PCM_CHANNELS AURORA_USB_CHANNELS_7_1_4
#define PCM_PERIOD_FRAMES AURORA_USB_PERIOD_FRAMES
#define PCM_PERIOD_SAMPLES (PCM_CHANNELS * PCM_PERIOD_FRAMES)
#define PCM_PERIOD_BYTES (PCM_PERIOD_SAMPLES * 4u)
#define TX_FRAME_CAP (AURORA_USB_HEADER_LEN + PCM_PERIOD_BYTES)
#define RX_FRAME_CAP AURORA_USB_MAX_FRAME
#define RENDER_READ_CHUNK (64u * 1024u)
#define RENDER_PENDING_CAP (128u * 1024u)
#define IO_TIMEOUT_MS 100

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
    int stdin_fd;
    int stdout_fd;
};

struct app {
    int bridge_fd;
    uint32_t tx_sequence;
    int configured;
    uint64_t next_pcm_pts;
    int have_pcm_pts;
    struct renderer_proc renderer;
    uint64_t renderer_serial;
    uint8_t render_pending[RENDER_PENDING_CAP];
    size_t render_pending_len;
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
    p[3] = (uint8_t)((v >> 24) & 0xffu);
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

static void renderer_close_fds(struct renderer_proc *r)
{
    if (r->stdin_fd >= 0) {
        close(r->stdin_fd);
        r->stdin_fd = -1;
    }
    if (r->stdout_fd >= 0) {
        close(r->stdout_fd);
        r->stdout_fd = -1;
    }
}

static void renderer_stop(struct renderer_proc *r)
{
    renderer_close_fds(r);
    if (r->pid <= 0)
        return;

    kill(r->pid, SIGTERM);
    for (int i = 0; i < 20; ++i) {
        pid_t w = waitpid(r->pid, NULL, WNOHANG);
        if (w == r->pid) {
            r->pid = -1;
            return;
        }
        usleep(10000);
    }
    kill(r->pid, SIGKILL);
    (void)waitpid(r->pid, NULL, 0);
    r->pid = -1;
}

static int renderer_start(struct renderer_proc *r)
{
    const char *orender = env_or("AURORA_ORENDER_BIN", DEFAULT_ORENDER);
    const char *bridge = env_or("AURORA_HARLETTY_BRIDGE", DEFAULT_HARLETTY);
    const char *layout = env_or("AURORA_7_1_4_LAYOUT", DEFAULT_LAYOUT);
    int to_child[2] = {-1, -1};
    int from_child[2] = {-1, -1};

    if (pipe2(to_child, O_CLOEXEC) < 0)
        return -1;
    if (pipe2(from_child, O_CLOEXEC) < 0) {
        close(to_child[0]);
        close(to_child[1]);
        return -1;
    }

    pid_t pid = fork();
    if (pid < 0) {
        close(to_child[0]); close(to_child[1]);
        close(from_child[0]); close(from_child[1]);
        return -1;
    }

    if (pid == 0) {
        if (dup2(to_child[0], STDIN_FILENO) < 0 ||
            dup2(from_child[1], STDOUT_FILENO) < 0)
            _exit(126);

        close(to_child[0]); close(to_child[1]);
        close(from_child[0]); close(from_child[1]);

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

    close(to_child[0]);
    close(from_child[1]);

    int flags = fcntl(to_child[1], F_GETFL, 0);
    if (flags >= 0)
        (void)fcntl(to_child[1], F_SETFL, flags | O_NONBLOCK);
    flags = fcntl(from_child[0], F_GETFL, 0);
    if (flags >= 0)
        (void)fcntl(from_child[0], F_SETFL, flags | O_NONBLOCK);

    r->pid = pid;
    r->stdin_fd = to_child[1];
    r->stdout_fd = from_child[0];
    fprintf(stderr, "aurora-live-ingest: orender pid=%ld\n", (long)pid);
    return 0;
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
    write_le32(frame + 12, a->tx_sequence++);
    write_le64(frame + 16, pts_48k);
    write_le32(frame + 24, payload_len);
    write_le32(frame + 28, aux);
    if (payload_len)
        memcpy(frame + AURORA_USB_HEADER_LEN, payload, payload_len);

    ssize_t n = send(a->bridge_fd, frame, total, MSG_NOSIGNAL);
    return n == (ssize_t)total ? 0 : -1;
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
        uint32_t flags = a->have_pcm_pts ? AURORA_USB_FLAG_PTS_VALID : 0;
        if (send_frame(a, AURORA_USB_PCM_S32LE, flags,
                       a->next_pcm_pts, aux, out, sizeof(out)) < 0)
            return -1;

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
    for (;;) {
        ssize_t n = read(a->renderer.stdout_fd, tmp, sizeof(tmp));
        if (n > 0) {
            if ((size_t)n > sizeof(a->render_pending) - a->render_pending_len) {
                fprintf(stderr, "aurora-live-ingest: rendered PCM backlog overflow\n");
                a->render_pending_len = 0;
                return -1;
            }
            memcpy(a->render_pending + a->render_pending_len, tmp, (size_t)n);
            a->render_pending_len += (size_t)n;
            if (flush_pcm_periods(a) < 0)
                return -1;
            continue;
        }
        if (n == 0)
            return 1;
        if (errno == EINTR)
            continue;
        if (errno == EAGAIN || errno == EWOULDBLOCK)
            return 0;
        return -1;
    }
}

static int validate_frame(const uint8_t *frame, size_t len,
                          uint16_t *kind, uint32_t *flags,
                          uint64_t *pts, const uint8_t **payload,
                          uint32_t *payload_len)
{
    if (len < AURORA_USB_HEADER_LEN ||
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
    a->render_pending_len = 0;
    a->next_pcm_pts = pts;
    a->have_pcm_pts = pts_valid;
    renderer_stop(&a->renderer);
    return renderer_start(&a->renderer);
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
        if ((flags & AURORA_USB_FLAG_DISCONTINUITY) != 0 &&
            restart_renderer(a, pts, (flags & AURORA_USB_FLAG_PTS_VALID) != 0) < 0)
            return -1;

        if (!a->have_pcm_pts && (flags & AURORA_USB_FLAG_PTS_VALID) != 0) {
            a->have_pcm_pts = 1;
            a->next_pcm_pts = pts;
        }

        /* Do NOT strip IEC61937 here. Omniphony v0.5.2 maintains a streaming
         * SpdifParser on stdin and forwards packet.payload + data_type to the
         * Harletty bridge as RInputTransport::Iec61937. */
        if (write_all_timeout(a->renderer.stdin_fd, payload, payload_len) < 0)
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
        }
        break;

    case AURORA_USB_CLOCK_REPORT:
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
    a.renderer.stdin_fd = -1;
    a.renderer.stdout_fd = -1;

    if (renderer_start(&a.renderer) < 0) {
        perror("aurora-live-ingest: start orender");
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
            if (send_config(&a) < 0) {
                close(a.bridge_fd);
                a.bridge_fd = -1;
                continue;
            }
            fprintf(stderr, "aurora-live-ingest: connected to %s\n",
                    env_or("AURORA_USB_BRIDGE_SOCKET", BRIDGE_SOCKET));
        }

        struct pollfd p[2];
        p[0] = (struct pollfd){.fd = a.bridge_fd, .events = POLLIN};
        p[1] = (struct pollfd){.fd = a.renderer.stdout_fd, .events = POLLIN};
        uint64_t renderer_serial_at_poll = a.renderer_serial;

        int rc = poll(p, 2, 250);
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
            ssize_t n = recv(a.bridge_fd, rx, RX_FRAME_CAP, 0);
            if (n <= 0) {
                close(a.bridge_fd);
                a.bridge_fd = -1;
                if (reset_after_bridge_disconnect(&a) < 0)
                    break;
                continue;
            }
            if (handle_bridge_frame(&a, rx, (size_t)n) < 0) {
                fprintf(stderr, "aurora-live-ingest: encoded input failure; restarting renderer\n");
                if (restart_renderer(&a, a.next_pcm_pts, a.have_pcm_pts) < 0)
                    break;
            }
        }

        /* If the bridge-side event restarted orender, revents for the old
         * descriptor are stale and must not be applied to the new renderer. */
        if (renderer_serial_at_poll != a.renderer_serial)
            continue;

        if (p[1].revents & POLLIN) {
            int rr = ingest_renderer_output(&a);
            if (rr < 0) {
                fprintf(stderr, "aurora-live-ingest: renderer output failure\n");
                if (restart_renderer(&a, a.next_pcm_pts, a.have_pcm_pts) < 0)
                    break;
            } else if (rr > 0) {
                if (restart_renderer(&a, a.next_pcm_pts, a.have_pcm_pts) < 0)
                    break;
            }
        }

        if (p[1].revents & (POLLHUP | POLLERR | POLLNVAL)) {
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
