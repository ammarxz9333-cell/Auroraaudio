#define _GNU_SOURCE

#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#include "aurora_source_manager_v1.h"

#define DEFAULT_SOCKET "/run/aurora/source-manager.sock"
#define MAX_CLIENTS 8
#define POLL_TIMEOUT_MS 50
#define QUIESCE_TIMEOUT_MS 250u

static volatile sig_atomic_t stop_requested;

struct client {
    int fd;
    uint16_t source;
    uint8_t registered;
    uint8_t present;
    uint8_t granted;
    uint32_t format;
};

struct manager {
    int listen_fd;
    struct client clients[MAX_CLIENTS];
    uint16_t active_source;
    uint16_t pending_source;
    uint16_t waiting_quiesce_source;
    uint32_t tx_sequence;
    uint64_t quiesce_deadline_ms;
    int64_t master_gain_mdb;
    uint32_t lipsync_frames;
    uint8_t muted;
    uint8_t standby;
};

static void on_signal(int signo)
{
    (void)signo;
    stop_requested = 1;
}

static uint64_t monotonic_ms(void)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0)
        return 0;
    return (uint64_t)ts.tv_sec * 1000u + (uint64_t)ts.tv_nsec / 1000000u;
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

static const char *socket_path(void)
{
    const char *path = getenv("AURORA_SOURCE_MANAGER_SOCKET");
    return (path && *path) ? path : DEFAULT_SOCKET;
}

static int source_priority(uint16_t source)
{
    switch (source) {
    case AURORA_SOURCE_HDMI_EARC: return 400;
    case AURORA_SOURCE_LOCAL_MUSIC: return 300;
    case AURORA_SOURCE_BLUETOOTH: return 200;
    case AURORA_SOURCE_NETWORK: return 100;
    default: return -1;
    }
}

static struct client *client_for_source(struct manager *m, uint16_t source)
{
    for (size_t i = 0; i < MAX_CLIENTS; ++i) {
        if (m->clients[i].fd >= 0 && m->clients[i].registered &&
            m->clients[i].source == source)
            return &m->clients[i];
    }
    return NULL;
}

static int send_message(struct manager *m, struct client *c, uint16_t kind,
                        uint16_t source, uint64_t data0,
                        uint32_t data1, uint32_t data2)
{
    uint8_t message[AURORA_SOURCE_MESSAGE_BYTES] = {0};
    if (!c || c->fd < 0)
        return -1;
    write_le32(message + 0, AURORA_SOURCE_MAGIC_U32);
    write_le16(message + 4, AURORA_SOURCE_VERSION);
    write_le16(message + 6, kind);
    write_le16(message + 8, source);
    write_le16(message + 10, 0);
    write_le32(message + 12, m->tx_sequence++);
    write_le64(message + 16, data0);
    write_le32(message + 24, data1);
    write_le32(message + 28, data2);
    ssize_t n = send(c->fd, message, sizeof(message), MSG_NOSIGNAL);
    return n == (ssize_t)sizeof(message) ? 0 : -1;
}

static void send_control_to(struct manager *m, struct client *c,
                            uint32_t control, uint64_t value)
{
    if (c && c->fd >= 0)
        (void)send_message(m, c, AURORA_SOURCE_CONTROL, c->source,
                           value, control, 0);
}

static void replay_controls(struct manager *m, struct client *c)
{
    send_control_to(m, c, AURORA_SOURCE_CTRL_MASTER_GAIN_MDB,
                    (uint64_t)m->master_gain_mdb);
    send_control_to(m, c, AURORA_SOURCE_CTRL_LIPSYNC_FRAMES,
                    m->lipsync_frames);
    send_control_to(m, c, AURORA_SOURCE_CTRL_STANDBY, m->standby);
    send_control_to(m, c, AURORA_SOURCE_CTRL_MUTE, m->muted);
}

static uint16_t best_present_source(struct manager *m)
{
    int best_priority = -1;
    uint16_t best_source = AURORA_SOURCE_NONE;
    for (size_t i = 0; i < MAX_CLIENTS; ++i) {
        struct client *c = &m->clients[i];
        if (c->fd < 0 || !c->registered || !c->present)
            continue;
        int priority = source_priority(c->source);
        if (priority > best_priority) {
            best_priority = priority;
            best_source = c->source;
        }
    }
    return best_source;
}

static void grant_source(struct manager *m, uint16_t source)
{
    struct client *c = client_for_source(m, source);
    if (!c || !c->present)
        return;
    c->granted = 1;
    m->active_source = source;
    m->pending_source = AURORA_SOURCE_NONE;
    m->waiting_quiesce_source = AURORA_SOURCE_NONE;
    m->quiesce_deadline_ms = 0;
    if (send_message(m, c, AURORA_SOURCE_GRANT, source, 0, 0, 0) < 0) {
        c->granted = 0;
        m->active_source = AURORA_SOURCE_NONE;
        return;
    }
    replay_controls(m, c);
    fprintf(stderr, "aurora-source-manager: grant source=%u\n", source);
}

static void reconcile(struct manager *m)
{
    uint16_t candidate = best_present_source(m);

    if (m->waiting_quiesce_source != AURORA_SOURCE_NONE) {
        m->pending_source = candidate;
        return;
    }
    if (candidate == m->active_source)
        return;
    if (m->active_source != AURORA_SOURCE_NONE) {
        struct client *active = client_for_source(m, m->active_source);
        if (!active) {
            m->active_source = AURORA_SOURCE_NONE;
            reconcile(m);
            return;
        }
        m->pending_source = candidate;
        m->waiting_quiesce_source = m->active_source;
        m->quiesce_deadline_ms = monotonic_ms() + QUIESCE_TIMEOUT_MS;
        if (send_message(m, active, AURORA_SOURCE_REVOKE,
                         active->source, AURORA_SOURCE_REVOKE_FADE_MS,
                         0, 0) < 0) {
            active->granted = 0;
            m->active_source = AURORA_SOURCE_NONE;
            m->waiting_quiesce_source = AURORA_SOURCE_NONE;
            m->quiesce_deadline_ms = 0;
            reconcile(m);
        }
        return;
    }
    if (candidate != AURORA_SOURCE_NONE)
        grant_source(m, candidate);
}

static int valid_source_registration(uint16_t source)
{
    return source_priority(source) >= 0 || source == AURORA_SOURCE_CONTROL_CLIENT;
}

static void disconnect_client(struct manager *m, struct client *c)
{
    if (!c || c->fd < 0)
        return;
    uint16_t source = c->source;
    int was_active = c->granted || source == m->active_source;
    int was_waiting = source == m->waiting_quiesce_source;
    close(c->fd);
    memset(c, 0, sizeof(*c));
    c->fd = -1;
    if (was_active)
        m->active_source = AURORA_SOURCE_NONE;
    if (was_waiting) {
        m->waiting_quiesce_source = AURORA_SOURCE_NONE;
        m->quiesce_deadline_ms = 0;
    }
    reconcile(m);
}

static void enforce_quiesce_deadline(struct manager *m)
{
    if (m->waiting_quiesce_source == AURORA_SOURCE_NONE ||
        m->quiesce_deadline_ms == 0)
        return;
    uint64_t now = monotonic_ms();
    if (now < m->quiesce_deadline_ms)
        return;

    uint16_t stuck_source = m->waiting_quiesce_source;
    struct client *stuck = client_for_source(m, stuck_source);
    fprintf(stderr, "aurora-source-manager: quiesce timeout source=%u\n",
            stuck_source);
    m->waiting_quiesce_source = AURORA_SOURCE_NONE;
    m->quiesce_deadline_ms = 0;
    if (stuck) {
        disconnect_client(m, stuck);
    } else {
        if (m->active_source == stuck_source)
            m->active_source = AURORA_SOURCE_NONE;
        reconcile(m);
    }
}

static int parse_message(const uint8_t *message, size_t len,
                         uint16_t *kind, uint16_t *source,
                         uint64_t *data0, uint32_t *data1, uint32_t *data2)
{
    if (len != AURORA_SOURCE_MESSAGE_BYTES ||
        read_le32(message + 0) != AURORA_SOURCE_MAGIC_U32 ||
        read_le16(message + 4) != AURORA_SOURCE_VERSION ||
        read_le16(message + 10) != 0)
        return -1;
    *kind = read_le16(message + 6);
    *source = read_le16(message + 8);
    *data0 = read_le64(message + 16);
    *data1 = read_le32(message + 24);
    *data2 = read_le32(message + 28);
    return 0;
}

static int handle_message(struct manager *m, struct client *c,
                          const uint8_t *message, size_t len)
{
    uint16_t kind, source;
    uint64_t data0;
    uint32_t data1, data2;
    if (parse_message(message, len, &kind, &source, &data0, &data1, &data2) < 0)
        return -1;
    (void)data2;

    if (kind == AURORA_SOURCE_REGISTER) {
        if (c->registered || !valid_source_registration(source))
            return -1;
        if (source != AURORA_SOURCE_CONTROL_CLIENT && client_for_source(m, source))
            return -1;
        c->source = source;
        c->registered = 1;
        return send_message(m, c, AURORA_SOURCE_STATUS, source,
                            m->active_source, 0, 0);
    }
    if (!c->registered || source != c->source)
        return -1;

    switch (kind) {
    case AURORA_SOURCE_PRESENT:
        if (source_priority(c->source) < 0)
            return -1;
        c->present = 1;
        reconcile(m);
        return 0;
    case AURORA_SOURCE_ABSENT:
        if (source_priority(c->source) < 0)
            return -1;
        c->present = 0;
        reconcile(m);
        return 0;
    case AURORA_SOURCE_FORMAT:
        if (source_priority(c->source) < 0)
            return -1;
        c->format = data1;
        return 0;
    case AURORA_SOURCE_QUIESCED:
        if (c->source != m->waiting_quiesce_source)
            return -1;
        c->granted = 0;
        m->active_source = AURORA_SOURCE_NONE;
        m->waiting_quiesce_source = AURORA_SOURCE_NONE;
        m->quiesce_deadline_ms = 0;
        reconcile(m);
        return 0;
    case AURORA_SOURCE_CONTROL:
        if (c->source != AURORA_SOURCE_CONTROL_CLIENT)
            return -1;
        switch (data1) {
        case AURORA_SOURCE_CTRL_MUTE:
            m->muted = data0 != 0;
            break;
        case AURORA_SOURCE_CTRL_MASTER_GAIN_MDB:
            m->master_gain_mdb = (int64_t)data0;
            break;
        case AURORA_SOURCE_CTRL_LIPSYNC_FRAMES:
            m->lipsync_frames = (uint32_t)data0;
            break;
        case AURORA_SOURCE_CTRL_STANDBY:
            m->standby = data0 != 0;
            break;
        default:
            return -1;
        }
        if (m->active_source != AURORA_SOURCE_NONE)
            send_control_to(m, client_for_source(m, m->active_source), data1, data0);
        return send_message(m, c, AURORA_SOURCE_STATUS,
                            AURORA_SOURCE_CONTROL_CLIENT,
                            m->active_source, data1, 0);
    default:
        return -1;
    }
}

static int open_listener(void)
{
    const char *path = socket_path();
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
        chmod(path, 0660) < 0 || listen(fd, MAX_CLIENTS) < 0) {
        int saved = errno;
        close(fd);
        unlink(path);
        errno = saved;
        return -1;
    }
    return fd;
}

int main(void)
{
    signal(SIGINT, on_signal);
    signal(SIGTERM, on_signal);
    signal(SIGPIPE, SIG_IGN);

    struct manager m;
    memset(&m, 0, sizeof(m));
    m.listen_fd = -1;
    for (size_t i = 0; i < MAX_CLIENTS; ++i)
        m.clients[i].fd = -1;

    m.listen_fd = open_listener();
    if (m.listen_fd < 0) {
        perror("aurora-source-manager: listen");
        return 1;
    }
    fprintf(stderr, "aurora-source-manager: listening on %s\n", socket_path());

    while (!stop_requested) {
        struct pollfd pfds[1 + MAX_CLIENTS];
        pfds[0] = (struct pollfd){.fd = m.listen_fd, .events = POLLIN};
        for (size_t i = 0; i < MAX_CLIENTS; ++i)
            pfds[i + 1] = (struct pollfd){.fd = m.clients[i].fd, .events = POLLIN};

        int rc = poll(pfds, 1 + MAX_CLIENTS, POLL_TIMEOUT_MS);
        if (rc < 0) {
            if (errno == EINTR)
                continue;
            perror("aurora-source-manager: poll");
            break;
        }
        if (pfds[0].revents & POLLIN) {
            int accepted = accept4(m.listen_fd, NULL, NULL, SOCK_CLOEXEC);
            if (accepted >= 0) {
                struct client *slot = NULL;
                for (size_t i = 0; i < MAX_CLIENTS; ++i) {
                    if (m.clients[i].fd < 0) {
                        slot = &m.clients[i];
                        break;
                    }
                }
                if (slot) {
                    memset(slot, 0, sizeof(*slot));
                    slot->fd = accepted;
                } else {
                    close(accepted);
                }
            }
        }

        for (size_t i = 0; i < MAX_CLIENTS; ++i) {
            struct client *c = &m.clients[i];
            if (c->fd < 0)
                continue;
            short revents = pfds[i + 1].revents;
            if (revents & (POLLHUP | POLLERR | POLLNVAL)) {
                disconnect_client(&m, c);
                continue;
            }
            if (revents & POLLIN) {
                uint8_t message[AURORA_SOURCE_MESSAGE_BYTES];
                ssize_t n = recv(c->fd, message, sizeof(message), 0);
                if (n <= 0 || handle_message(&m, c, message, (size_t)n) < 0)
                    disconnect_client(&m, c);
            }
        }
        enforce_quiesce_deadline(&m);
    }

    for (size_t i = 0; i < MAX_CLIENTS; ++i)
        if (m.clients[i].fd >= 0)
            close(m.clients[i].fd);
    close(m.listen_fd);
    unlink(socket_path());
    return 0;
}