#define _DEFAULT_SOURCE

#include <linux/usb/ch9.h>
#include <linux/usb/functionfs.h>

#include <errno.h>
#include <fcntl.h>
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
#include <unistd.h>

#include "aurora_usb_stream_v1.h"
#include "aurora_usb_v1.h"

#define FFS_DIR "/dev/ffs-aurora"
#define EP0_PATH FFS_DIR "/ep0"
#define EP1_PATH FFS_DIR "/ep1"
#define EP2_PATH FFS_DIR "/ep2"
#define RUN_DIR "/run/aurora"
#define BRIDGE_SOCKET RUN_DIR "/usb-bridge.sock"
#define USB_READ_CHUNK (64u * 1024u)
#define USB_WRITE_TIMEOUT_MS 100

struct ffs_desc_blob {
    struct usb_functionfs_descs_head_v2 header;
    uint32_t fs_count;
    uint32_t hs_count;

    struct usb_interface_descriptor fs_intf;
    struct usb_endpoint_descriptor_no_audio fs_out;
    struct usb_endpoint_descriptor_no_audio fs_in;

    struct usb_interface_descriptor hs_intf;
    struct usb_endpoint_descriptor_no_audio hs_out;
    struct usb_endpoint_descriptor_no_audio hs_in;
} __attribute__((packed));

struct frame_dispatch_ctx {
    int backend_fd;
    int ep_in;
};

static volatile sig_atomic_t stop_requested;
static uint32_t tx_sequence;

static void on_signal(int signo)
{
    (void)signo;
    stop_requested = 1;
}

static int wait_writable(int fd)
{
    struct pollfd p = { .fd = fd, .events = POLLOUT };
    for (;;) {
        int rc = poll(&p, 1, USB_WRITE_TIMEOUT_MS);
        if (rc > 0)
            return (p.revents & POLLOUT) ? 0 : -1;
        if (rc == 0) {
            errno = ETIMEDOUT;
            return -1;
        }
        if (errno != EINTR)
            return -1;
    }
}

static int write_all(int fd, const void *buf, size_t len)
{
    const uint8_t *p = buf;
    while (len > 0) {
        ssize_t n = write(fd, p, len);
        if (n < 0) {
            if (errno == EINTR)
                continue;
            if ((errno == EAGAIN || errno == EWOULDBLOCK) && wait_writable(fd) == 0)
                continue;
            return -1;
        }
        if (n == 0) {
            errno = EIO;
            return -1;
        }
        p += (size_t)n;
        len -= (size_t)n;
    }
    return 0;
}

static void fill_interface(struct usb_interface_descriptor *d)
{
    memset(d, 0, sizeof(*d));
    d->bLength = sizeof(*d);
    d->bDescriptorType = USB_DT_INTERFACE;
    d->bInterfaceNumber = 0;
    d->bAlternateSetting = 0;
    d->bNumEndpoints = 2;
    d->bInterfaceClass = USB_CLASS_VENDOR_SPEC;
    d->bInterfaceSubClass = 0x41; /* 'A' - Aurora private protocol */
    d->bInterfaceProtocol = AURORA_USB_VERSION;
    d->iInterface = 1;
}

static void fill_endpoint(struct usb_endpoint_descriptor_no_audio *d,
                          uint8_t addr, uint16_t max_packet)
{
    memset(d, 0, sizeof(*d));
    d->bLength = sizeof(*d);
    d->bDescriptorType = USB_DT_ENDPOINT;
    d->bEndpointAddress = addr;
    d->bmAttributes = USB_ENDPOINT_XFER_BULK;
    d->wMaxPacketSize = htole16(max_packet);
    d->bInterval = 0;
}

static int register_descriptors(int ep0)
{
    struct ffs_desc_blob d;
    memset(&d, 0, sizeof(d));

    d.header.magic = htole32(FUNCTIONFS_DESCRIPTORS_MAGIC_V2);
    d.header.length = htole32((uint32_t)sizeof(d));
    d.header.flags = htole32(FUNCTIONFS_HAS_FS_DESC | FUNCTIONFS_HAS_HS_DESC);
    d.fs_count = htole32(3);
    d.hs_count = htole32(3);

    fill_interface(&d.fs_intf);
    fill_endpoint(&d.fs_out, 0x01, 64);
    fill_endpoint(&d.fs_in, USB_DIR_IN | 0x02, 64);

    fill_interface(&d.hs_intf);
    fill_endpoint(&d.hs_out, 0x01, 512);
    fill_endpoint(&d.hs_in, USB_DIR_IN | 0x02, 512);

    if (write_all(ep0, &d, sizeof(d)) < 0)
        return -1;

    static const char interface_name[] = "Aurora S6 realtime transport";
    const size_t total = sizeof(struct usb_functionfs_strings_head) +
                         sizeof(uint16_t) + sizeof(interface_name);
    uint8_t *blob = calloc(1, total);
    if (!blob)
        return -1;

    struct usb_functionfs_strings_head *h = (void *)blob;
    h->magic = htole32(FUNCTIONFS_STRINGS_MAGIC);
    h->length = htole32((uint32_t)total);
    h->str_count = htole32(1);
    h->lang_count = htole32(1);

    uint16_t lang = htole16(0x0409);
    memcpy(blob + sizeof(*h), &lang, sizeof(lang));
    memcpy(blob + sizeof(*h) + sizeof(lang), interface_name,
           sizeof(interface_name));

    const int rc = write_all(ep0, blob, total);
    free(blob);
    return rc;
}

static int mkdir_if_needed(const char *path, mode_t mode)
{
    if (mkdir(path, mode) == 0 || errno == EEXIST)
        return 0;
    return -1;
}

static int make_bridge_listener(void)
{
    if (mkdir_if_needed(RUN_DIR, 0755) < 0)
        return -1;

    int fd = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (fd < 0)
        return -1;

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    if (strlen(BRIDGE_SOCKET) >= sizeof(addr.sun_path)) {
        close(fd);
        errno = ENAMETOOLONG;
        return -1;
    }
    strcpy(addr.sun_path, BRIDGE_SOCKET);
    unlink(BRIDGE_SOCKET);

    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0 ||
        listen(fd, 1) < 0) {
        int saved = errno;
        close(fd);
        errno = saved;
        return -1;
    }
    chmod(BRIDGE_SOCKET, 0660);
    return fd;
}

static int validate_complete_frame(const uint8_t *buf, size_t len, uint16_t *kind)
{
    if (len < AURORA_USB_HEADER_LEN)
        return -1;
    if (aurora_usb_read_le32(buf) != AURORA_USB_MAGIC_U32)
        return -1;
    if (aurora_usb_read_le16(buf + 4) != AURORA_USB_VERSION)
        return -1;

    uint32_t payload_len = aurora_usb_read_le32(buf + 24);
    if (payload_len > AURORA_USB_MAX_FRAME - AURORA_USB_HEADER_LEN)
        return -1;
    if ((size_t)payload_len + AURORA_USB_HEADER_LEN != len)
        return -1;

    if (kind)
        *kind = aurora_usb_read_le16(buf + 6);
    return 0;
}

static int send_control_frame(int ep_in, uint16_t kind,
                              const void *payload, uint32_t payload_len)
{
    if (payload_len > AURORA_USB_MAX_FRAME - AURORA_USB_HEADER_LEN) {
        errno = EMSGSIZE;
        return -1;
    }

    const size_t total = AURORA_USB_HEADER_LEN + (size_t)payload_len;
    uint8_t *frame = calloc(1, total);
    if (!frame)
        return -1;

    aurora_usb_write_le32(frame + 0, AURORA_USB_MAGIC_U32);
    aurora_usb_write_le16(frame + 4, AURORA_USB_VERSION);
    aurora_usb_write_le16(frame + 6, kind);
    aurora_usb_write_le32(frame + 12, tx_sequence++);
    aurora_usb_write_le32(frame + 24, payload_len);
    if (payload_len)
        memcpy(frame + AURORA_USB_HEADER_LEN, payload, payload_len);

    const int rc = write_all(ep_in, frame, total);
    free(frame);
    return rc;
}

static int dispatch_usb_frame(void *opaque, const uint8_t *frame, size_t len)
{
    struct frame_dispatch_ctx *ctx = opaque;
    uint16_t kind = 0;
    if (validate_complete_frame(frame, len, &kind) < 0)
        return -1;

    if (kind == AURORA_USB_PING)
        return send_control_frame(ctx->ep_in, AURORA_USB_PONG, NULL, 0);

    if (ctx->backend_fd < 0) {
        static const char offline[] = "backend-offline";
        (void)send_control_frame(ctx->ep_in, AURORA_USB_ERROR,
                                 offline, (uint32_t)(sizeof(offline) - 1));
        return 0;
    }

    ssize_t sent = send(ctx->backend_fd, frame, len, MSG_NOSIGNAL);
    if (sent != (ssize_t)len)
        return -1;
    return 0;
}

static void close_endpoint(int *fd)
{
    if (*fd >= 0) {
        close(*fd);
        *fd = -1;
    }
}

static int open_stream_endpoints(int *ep_out, int *ep_in)
{
    close_endpoint(ep_out);
    close_endpoint(ep_in);

    *ep_out = open(EP1_PATH, O_RDONLY | O_CLOEXEC | O_NONBLOCK);
    if (*ep_out < 0)
        return -1;
    *ep_in = open(EP2_PATH, O_WRONLY | O_CLOEXEC | O_NONBLOCK);
    if (*ep_in < 0) {
        close_endpoint(ep_out);
        return -1;
    }
    return 0;
}

static void handle_ep0_events(int ep0, int *enabled, int *ep_out, int *ep_in,
                              int *backend_fd,
                              struct aurora_usb_stream_v1 *stream)
{
    struct usb_functionfs_event events[8];
    ssize_t n = read(ep0, events, sizeof(events));
    if (n <= 0)
        return;

    size_t count = (size_t)n / sizeof(events[0]);
    for (size_t i = 0; i < count; ++i) {
        switch (events[i].type) {
        case FUNCTIONFS_ENABLE:
            if (open_stream_endpoints(ep_out, ep_in) == 0) {
                aurora_usb_stream_v1_reset(stream);
                tx_sequence = 0;
                *enabled = 1;
                fprintf(stderr, "aurora-ffs: USB enabled\n");
            } else {
                perror("aurora-ffs: opening stream endpoints");
            }
            break;
        case FUNCTIONFS_DISABLE:
        case FUNCTIONFS_UNBIND:
            *enabled = 0;
            aurora_usb_stream_v1_reset(stream);
            close_endpoint(ep_out);
            close_endpoint(ep_in);
            /* A USB session boundary invalidates STM32 CONFIG state. Force the
             * live broker to reconnect so it must send a fresh CONFIG before
             * any new PCM can reach the next USB session. */
            close_endpoint(backend_fd);
            fprintf(stderr, "aurora-ffs: USB disabled; backend reset required\n");
            break;
        case FUNCTIONFS_SETUP:
            /* Protocol v1 defines no class/vendor ep0 request. */
            break;
        default:
            break;
        }
    }
}

int main(void)
{
    signal(SIGINT, on_signal);
    signal(SIGTERM, on_signal);
    signal(SIGPIPE, SIG_IGN);

    int ep0 = open(EP0_PATH, O_RDWR | O_CLOEXEC | O_NONBLOCK);
    if (ep0 < 0) {
        perror("aurora-ffs: open ep0");
        return 1;
    }
    if (register_descriptors(ep0) < 0) {
        perror("aurora-ffs: register descriptors");
        close(ep0);
        return 1;
    }

    int listen_fd = make_bridge_listener();
    if (listen_fd < 0) {
        perror("aurora-ffs: bridge listener");
        close(ep0);
        return 1;
    }

    int backend_fd = -1;
    int ep_out = -1;
    int ep_in = -1;
    int usb_enabled = 0;

    uint8_t *frame = malloc(AURORA_USB_MAX_FRAME);
    uint8_t *stream_storage = malloc(AURORA_USB_MAX_FRAME);
    uint8_t *usb_chunk = malloc(USB_READ_CHUNK);
    if (!frame || !stream_storage || !usb_chunk) {
        perror("aurora-ffs: buffers");
        free(frame);
        free(stream_storage);
        free(usb_chunk);
        close(listen_fd);
        close(ep0);
        return 1;
    }

    struct aurora_usb_stream_v1 stream;
    aurora_usb_stream_v1_init(&stream, stream_storage, AURORA_USB_MAX_FRAME);

    fprintf(stderr, "aurora-ffs: registered; backend socket %s\n", BRIDGE_SOCKET);

    while (!stop_requested) {
        struct pollfd p[4];
        p[0] = (struct pollfd){ .fd = ep0, .events = POLLIN };
        /* Do not accept a live backend until USB is enabled. A broker may
         * connect into the Unix-listener backlog while USB is down; accepting
         * it only after ENABLE guarantees its first CONFIG belongs to the new
         * physical USB session. */
        p[1] = (struct pollfd){ .fd = usb_enabled ? listen_fd : -1, .events = POLLIN };
        p[2] = (struct pollfd){ .fd = usb_enabled ? ep_out : -1, .events = POLLIN };
        p[3] = (struct pollfd){ .fd = backend_fd, .events = POLLIN };

        int rc = poll(p, 4, 500);
        if (rc < 0) {
            if (errno == EINTR)
                continue;
            perror("aurora-ffs: poll");
            break;
        }

        if (p[0].revents & POLLIN)
            handle_ep0_events(ep0, &usb_enabled, &ep_out, &ep_in,
                              &backend_fd, &stream);

        if (p[1].revents & POLLIN) {
            int fd = accept4(listen_fd, NULL, NULL, SOCK_CLOEXEC | SOCK_NONBLOCK);
            if (fd >= 0) {
                close_endpoint(&backend_fd);
                backend_fd = fd;
                fprintf(stderr, "aurora-ffs: backend connected\n");
            }
        }

        if (usb_enabled && ep_out >= 0 && (p[2].revents & POLLIN)) {
            ssize_t n = read(ep_out, usb_chunk, USB_READ_CHUNK);
            if (n > 0) {
                struct frame_dispatch_ctx ctx = {
                    .backend_fd = backend_fd,
                    .ep_in = ep_in,
                };
                int feed_rc = aurora_usb_stream_v1_feed(
                    &stream, usb_chunk, (size_t)n, dispatch_usb_frame, &ctx);
                if (feed_rc != AURORA_USB_STREAM_OK) {
                    static const char bad[] = "usb-stream-framing-error";
                    aurora_usb_stream_v1_reset(&stream);
                    (void)send_control_frame(ep_in, AURORA_USB_ERROR,
                                             bad, (uint32_t)(sizeof(bad) - 1));
                    if (feed_rc == AURORA_USB_STREAM_ERR_CALLBACK && backend_fd >= 0) {
                        close_endpoint(&backend_fd);
                        fprintf(stderr, "aurora-ffs: backend disconnected during USB dispatch\n");
                    }
                }
            }
        }

        if (backend_fd >= 0 && (p[3].revents & (POLLIN | POLLHUP | POLLERR))) {
            if (p[3].revents & POLLIN) {
                ssize_t n = recv(backend_fd, frame, AURORA_USB_MAX_FRAME, 0);
                if (n > 0 && usb_enabled && ep_in >= 0) {
                    if (validate_complete_frame(frame, (size_t)n, NULL) == 0) {
                        if (write_all(ep_in, frame, (size_t)n) < 0) {
                            perror("aurora-ffs: ep2 write");
                            usb_enabled = 0;
                            close_endpoint(&ep_out);
                            close_endpoint(&ep_in);
                            aurora_usb_stream_v1_reset(&stream);
                            close_endpoint(&backend_fd);
                        }
                    }
                } else if (n == 0) {
                    close_endpoint(&backend_fd);
                }
            }
            if (p[3].revents & (POLLHUP | POLLERR))
                close_endpoint(&backend_fd);
        }
    }

    free(frame);
    free(stream_storage);
    free(usb_chunk);
    close_endpoint(&backend_fd);
    close_endpoint(&ep_out);
    close_endpoint(&ep_in);
    close(listen_fd);
    unlink(BRIDGE_SOCKET);
    close(ep0);
    return 0;
}
