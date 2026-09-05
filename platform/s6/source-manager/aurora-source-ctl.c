#define _GNU_SOURCE

#include <errno.h>
#include <math.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include "aurora_source_manager_v1.h"

#define DEFAULT_SOCKET "/run/aurora/source-manager.sock"
#define ACK_TIMEOUT_MS 500

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

static int connect_manager(void)
{
    int fd = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return -1;

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    const char *path = socket_path();
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

static int send_message(int fd, uint16_t kind, uint64_t data0, uint32_t data1)
{
    uint8_t message[AURORA_SOURCE_MESSAGE_BYTES] = {0};
    write_le32(message + 0, AURORA_SOURCE_MAGIC_U32);
    write_le16(message + 4, AURORA_SOURCE_VERSION);
    write_le16(message + 6, kind);
    write_le16(message + 8, AURORA_SOURCE_CONTROL_CLIENT);
    write_le16(message + 10, 0);
    write_le32(message + 12, 0);
    write_le64(message + 16, data0);
    write_le32(message + 24, data1);
    write_le32(message + 28, 0);
    ssize_t n = send(fd, message, sizeof(message), MSG_NOSIGNAL);
    return n == (ssize_t)sizeof(message) ? 0 : -1;
}

static int recv_status(int fd, uint32_t expected_control,
                       uint16_t *active_source)
{
    struct pollfd p = {.fd = fd, .events = POLLIN};
    int rc = poll(&p, 1, ACK_TIMEOUT_MS);
    if (rc <= 0 || !(p.revents & POLLIN))
        return -1;

    uint8_t message[AURORA_SOURCE_MESSAGE_BYTES];
    ssize_t n = recv(fd, message, sizeof(message), 0);
    if (n != (ssize_t)sizeof(message) ||
        read_le32(message + 0) != AURORA_SOURCE_MAGIC_U32 ||
        read_le16(message + 4) != AURORA_SOURCE_VERSION ||
        read_le16(message + 6) != AURORA_SOURCE_STATUS ||
        read_le16(message + 8) != AURORA_SOURCE_CONTROL_CLIENT ||
        read_le16(message + 10) != 0 ||
        read_le32(message + 24) != expected_control)
        return -1;

    uint64_t active = read_le64(message + 16);
    if (active > UINT16_MAX)
        return -1;
    *active_source = (uint16_t)active;
    return 0;
}

static const char *source_name(uint16_t source)
{
    switch (source) {
    case AURORA_SOURCE_NONE: return "none";
    case AURORA_SOURCE_HDMI_EARC: return "hdmi-earc";
    case AURORA_SOURCE_LOCAL_MUSIC: return "local-music";
    case AURORA_SOURCE_BLUETOOTH: return "bluetooth";
    case AURORA_SOURCE_NETWORK: return "network";
    default: return "unknown";
    }
}

static int parse_on_off(const char *text, uint64_t *value)
{
    if (strcmp(text, "on") == 0 || strcmp(text, "1") == 0) {
        *value = 1;
        return 0;
    }
    if (strcmp(text, "off") == 0 || strcmp(text, "0") == 0) {
        *value = 0;
        return 0;
    }
    return -1;
}

static int parse_gain_db(const char *text, uint64_t *value)
{
    char *end = NULL;
    errno = 0;
    double db = strtod(text, &end);
    if (errno != 0 || !end || *end != '\0' || !isfinite(db) || db < -80.0 || db > 0.0)
        return -1;
    int64_t milli_db = (int64_t)llround(db * 1000.0);
    *value = (uint64_t)milli_db;
    return 0;
}

static int parse_lipsync_ms(const char *text, uint64_t *value)
{
    char *end = NULL;
    errno = 0;
    double ms = strtod(text, &end);
    if (errno != 0 || !end || *end != '\0' || !isfinite(ms) || ms < 0.0 ||
        ms > (double)AURORA_SOURCE_MAX_LIPSYNC_FRAMES / 48.0)
        return -1;
    double frames = ms * 48.0;
    *value = (uint64_t)llround(frames);
    return 0;
}

static void usage(const char *argv0)
{
    fprintf(stderr,
            "usage:\n"
            "  %s status\n"
            "  %s mute on|off\n"
            "  %s gain-db -80..0\n"
            "  %s lipsync-ms 0..500\n"
            "  %s standby on|off\n",
            argv0, argv0, argv0, argv0, argv0);
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        usage(argv[0]);
        return 2;
    }

    int fd = connect_manager();
    if (fd < 0) {
        perror("aurora-source-ctl: connect");
        return 1;
    }

    if (send_message(fd, AURORA_SOURCE_REGISTER, 0, 0) < 0) {
        perror("aurora-source-ctl: register");
        close(fd);
        return 1;
    }

    uint16_t active_source = AURORA_SOURCE_NONE;
    if (recv_status(fd, 0, &active_source) < 0) {
        fprintf(stderr, "aurora-source-ctl: invalid or missing registration status\n");
        close(fd);
        return 1;
    }

    if (strcmp(argv[1], "status") == 0) {
        if (argc != 2) {
            usage(argv[0]);
            close(fd);
            return 2;
        }
        printf("active_source=%s\n", source_name(active_source));
        close(fd);
        return 0;
    }

    uint32_t control = 0;
    uint64_t value = 0;
    if (strcmp(argv[1], "mute") == 0 && argc == 3) {
        control = AURORA_SOURCE_CTRL_MUTE;
        if (parse_on_off(argv[2], &value) < 0) {
            usage(argv[0]);
            close(fd);
            return 2;
        }
    } else if (strcmp(argv[1], "gain-db") == 0 && argc == 3) {
        control = AURORA_SOURCE_CTRL_MASTER_GAIN_MDB;
        if (parse_gain_db(argv[2], &value) < 0) {
            usage(argv[0]);
            close(fd);
            return 2;
        }
    } else if (strcmp(argv[1], "lipsync-ms") == 0 && argc == 3) {
        control = AURORA_SOURCE_CTRL_LIPSYNC_FRAMES;
        if (parse_lipsync_ms(argv[2], &value) < 0) {
            usage(argv[0]);
            close(fd);
            return 2;
        }
    } else if (strcmp(argv[1], "standby") == 0 && argc == 3) {
        control = AURORA_SOURCE_CTRL_STANDBY;
        if (parse_on_off(argv[2], &value) < 0) {
            usage(argv[0]);
            close(fd);
            return 2;
        }
    } else {
        usage(argv[0]);
        close(fd);
        return 2;
    }

    if (send_message(fd, AURORA_SOURCE_CONTROL, value, control) < 0) {
        perror("aurora-source-ctl: control");
        close(fd);
        return 1;
    }

    if (recv_status(fd, control, &active_source) < 0) {
        fprintf(stderr, "aurora-source-ctl: control was not acknowledged by source manager\n");
        close(fd);
        return 1;
    }

    close(fd);
    return 0;
}
