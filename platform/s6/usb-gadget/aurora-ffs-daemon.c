#define _DEFAULT_SOURCE

#include <linux/usb/ch9.h>
#include <linux/usb/functionfs.h>

#include <endian.h>
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

#define FFS_DIR "/dev/ffs-aurora"
#define EP0_PATH FFS_DIR "/ep0"
#define EP1_PATH FFS_DIR "/ep1"
#define EP2_PATH FFS_DIR "/ep2"
#define RUN_DIR "/run/aurora"
#define BRIDGE_SOCKET RUN_DIR "/usb-bridge.sock"

#define AURORA_MAGIC "AUR0"
#define AURORA_VERSION 1u
#define AURORA_HEADER_LEN 32u
#define AURORA_KIND_ERROR 6u
#define AURORA_KIND_PING 7u
#define AURORA_KIND_PONG 8u
#define MAX_FRAME (256u * 1024u)

struct aurora_header_wire {
    uint8_t magic[4];
    uint16_t version;
    uint16_t kind;
    uint32_t flags;
    uint32_t sequence;
    uint64_t pts_48k;
    uint32_t payload_len;
    uint32_t aux;
} __attribute__((packed));

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

static volatile sig_atomic_t stop_requested;
static uint32_t tx_sequence;

static void on_signal(int signo)
{
    (void)signo;
    stop_requested = 1;
}

static int write_all(int fd, const void *buf, size_t len)
{
    const uint8_t *p = buf;
    while (len > 0) {
        ssize_t n = write(fd, p, len);
        if (n < 0) {
            if (errno == EINTR)
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
    d->bInterfaceProtocol = 1;
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

static int validate_frame(const uint8_t *buf, size_t len, uint16_t *kind)
{
    if (len < AURORA_HEADER_LEN)
        return -1;

    const struct aurora_header_wire *h = (const void *)buf;
    if (memcmp(h->magic, AURORA_MAGIC, 4) != 0)
        return -1;
    if (le16toh(h->version) != AURORA_VERSION)
        return -1;

    uint32_t payload_len = le32toh(h->payload_len);
    if ((size_t)payload_len != len - AURORA_HEADER_LEN)
        return -1;

    if (kind)
        *kind = le16toh(h->kind);
    return 0;
}

static int send_control_frame(int ep_in, uint16_t kind,
                              const void *payload, uint32_t payload_len)
{
    if (payload_len > MAX_FRAME - AURORA_HEADER_LEN) {
        errno = EMSGSIZE;
        return -1;
    }

    uint8_t *frame = calloc(1, AURORA_HEADER_LEN + payload_len);
    if (!frame)
        return -1;

    struct aurora_header_wire *h = (void *)frame;
    memcpy(h->magic, AURORA_MAGIC, 4);
    h->version = htole16(AURORA_VERSION);
    h->kind = htole16(kind);
    h->sequence = htole32(tx_sequence++);
    h->payload_len = htole32(payload_len);
    if (payload_len)
        memcpy(frame + AURORA_HEADER_LEN, payload, payload_len);

    const int rc = write_all(ep_in, frame, AURORA_HEADER_LEN + payload_len);
    free(frame);
    return rc;
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

static void handle_ep0_events(int ep0, int *enabled, int *ep_out, int *ep_in)
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
                *enabled = 1;
                fprintf(stderr, "aurora-ffs: USB enabled\n");
            } else {
                perror("aurora-ffs: opening stream endpoints");
            }
            break;
        case FUNCTIONFS_DISABLE:
        case FUNCTIONFS_UNBIND:
            *enabled = 0;
            close_endpoint(ep_out);
            close_endpoint(ep_in);
            fprintf(stderr, "aurora-ffs: USB disabled\n");
            break;
        case FUNCTIONFS_SETUP:
            /* Aurora defines no class/vendor control requests in v1. The
             * realtime protocol is carried exclusively over bulk endpoints. */
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
    uint8_t *frame = malloc(MAX_FRAME);
    if (!frame) {
        perror("aurora-ffs: frame buffer");
        close(listen_fd);
        close(ep0);
        return 1;
    }

    fprintf(stderr, "aurora-ffs: registered; backend socket %s\n", BRIDGE_SOCKET);

    while (!stop_requested) {
        struct pollfd p[4];
        p[0] = (struct pollfd){ .fd = ep0, .events = POLLIN };
        p[1] = (struct pollfd){ .fd = listen_fd, .events = POLLIN };
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
            handle_ep0_events(ep0, &usb_enabled, &ep_out, &ep_in);

        if (p[1].revents & POLLIN) {
            int fd = accept4(listen_fd, NULL, NULL, SOCK_CLOEXEC | SOCK_NONBLOCK);
            if (fd >= 0) {
                close_endpoint(&backend_fd);
                backend_fd = fd;
                fprintf(stderr, "aurora-ffs: backend connected\n");
            }
        }

        if (usb_enabled && ep_out >= 0 && (p[2].revents & POLLIN)) {
            ssize_t n = read(ep_out, frame, MAX_FRAME);
            if (n > 0) {
                uint16_t kind = 0;
                if (validate_frame(frame, (size_t)n, &kind) < 0) {
                    static const char bad[] = "malformed-frame";
                    (void)send_control_frame(ep_in, AURORA_KIND_ERROR,
                                             bad, (uint32_t)(sizeof(bad) - 1));
                } else if (kind == AURORA_KIND_PING) {
                    (void)send_control_frame(ep_in, AURORA_KIND_PONG, NULL, 0);
                } else if (backend_fd >= 0) {
                    ssize_t sent = send(backend_fd, frame, (size_t)n, MSG_NOSIGNAL);
                    if (sent != n) {
                        close_endpoint(&backend_fd);
                        fprintf(stderr, "aurora-ffs: backend disconnected during send\n");
                    }
                } else {
                    static const char offline[] = "backend-offline";
                    (void)send_control_frame(ep_in, AURORA_KIND_ERROR,
                                             offline,
                                             (uint32_t)(sizeof(offline) - 1));
                }
            }
        }

        if (backend_fd >= 0 && (p[3].revents & (POLLIN | POLLHUP | POLLERR))) {
            if (p[3].revents & POLLIN) {
                ssize_t n = recv(backend_fd, frame, MAX_FRAME, 0);
                if (n > 0 && usb_enabled && ep_in >= 0) {
                    if (validate_frame(frame, (size_t)n, NULL) == 0) {
                        if (write_all(ep_in, frame, (size_t)n) < 0 &&
                            errno != EAGAIN && errno != EWOULDBLOCK) {
                            perror("aurora-ffs: ep2 write");
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
    close_endpoint(&backend_fd);
    close_endpoint(&ep_out);
    close_endpoint(&ep_in);
    close(listen_fd);
    unlink(BRIDGE_SOCKET);
    close(ep0);
    return 0;
}
