#define _GNU_SOURCE
#include <assert.h>
#include <sys/socket.h>
#include <time.h>

/* Exercise real nonblocking pipes. Only the downstream USB socket send is
 * substituted: this test is software transport evidence, never codec proof. */
static ssize_t test_send(int fd, const void *buf, size_t len, int flags);
#define send test_send
#define main broker_main
#include "aurora-live-ingest.c"
#undef main
#undef send

static int blocked;
static uint32_t packets;
static uint64_t first_pts;

static ssize_t test_send(int fd, const void *buf, size_t len, int flags)
{
    (void)fd;
    assert(flags & MSG_DONTWAIT);
    if (blocked) {
        errno = EAGAIN;
        return -1;
    }
    const uint8_t *p = buf;
    assert(len == TX_FRAME_CAP);
    assert(read_le32(p + 12) == packets);
    assert(read_le64(p + 16) == first_pts + packets * PCM_PERIOD_FRAMES);
    for (size_t i = AURORA_USB_HEADER_LEN; i < len; i += 4)
        assert(read_le32(p + i) == 536870912u);
    packets++;
    return (ssize_t)len;
}

static void init_app(struct app *a)
{
    memset(a, 0, sizeof(*a));
    a->configured = 1;
    a->have_pcm_pts = 1;
    a->next_pcm_pts = first_pts = 48000;
    packets = 0;
    blocked = 0;
}

static void fill_pcm(uint8_t *p, size_t count)
{
    float sample = 0.25f;
    for (size_t i = 0; i < count; i += sizeof(sample))
        memcpy(p + i, &sample, sizeof(sample));
}

static void test_output_retry(void)
{
    struct app a;
    init_app(&a);
    fill_pcm(a.render_pending, PCM_PERIOD_BYTES);
    a.render_pending_len = PCM_PERIOD_BYTES;
    a.next_pcm_flags = AURORA_USB_FLAG_DISCONTINUITY;
    blocked = 1;
    assert(flush_pcm_periods(&a) == 0);
    assert(a.render_pending_len == PCM_PERIOD_BYTES);
    assert(a.tx_sequence == 0 && a.next_pcm_pts == first_pts);
    assert(a.next_pcm_flags == AURORA_USB_FLAG_DISCONTINUITY);
    blocked = 0;
    assert(flush_pcm_periods(&a) == 0);
    assert(packets == 1 && a.render_pending_len == 0);
    assert(a.tx_sequence == 1 && a.next_pcm_flags == 0);
}

static void test_queue_capacity(void)
{
    struct app a;
    init_app(&a);
    int p[2];
    assert(pipe2(p, O_NONBLOCK) == 0);
    uint8_t chunk[4096] = {0};
    while (write(p[1], chunk, sizeof(chunk)) > 0) {}
    assert(errno == EAGAIN);
    a.renderer.stdin_fd = p[1];
    /* Full pipe must leave all bytes queued, without polling or timing out. */
    assert(queue_encoded(&a, chunk, sizeof(chunk)) == 0);
    assert(a.encoded_pending_len == sizeof(chunk));
    a.encoded_pending_offset = ENCODED_PENDING_CAP - sizeof(chunk);
    memset(a.encoded_pending + a.encoded_pending_offset, 0xaa, sizeof(chunk));
    memset(chunk, 0xbb, sizeof(chunk));
    assert(queue_encoded(&a, chunk, sizeof(chunk)) == 0);
    assert(a.encoded_pending_offset == 0);
    assert(a.encoded_pending_len == 2 * sizeof(chunk));
    for (size_t i = 0; i < sizeof(chunk); ++i) {
        assert(a.encoded_pending[i] == 0xaa);
        assert(a.encoded_pending[i + sizeof(chunk)] == 0xbb);
    }
    a.encoded_pending_len = ENCODED_PENDING_CAP;
    assert(queue_encoded(&a, chunk, 1) == -1 && errno == ENOBUFS);
    assert(a.encoded_pending_len == ENCODED_PENDING_CAP);
    close(p[0]);
    close(p[1]);
}

static void test_full_output_queue(void)
{
    struct app a;
    init_app(&a);
    int p[2];
    assert(pipe2(p, O_NONBLOCK) == 0);
    a.renderer.stdout_fd = p[0];
    a.render_pending_len = RENDER_PENDING_CAP;
    assert(write(p[1], "abcd", 4) == 4);
    assert(ingest_renderer_output(&a) == 0);
    assert(a.render_pending_len == RENDER_PENDING_CAP);
    char buf[4];
    assert(read(p[0], buf, 4) == 4); /* No read/drop while queue is full. */
    close(p[0]);
    close(p[1]);
}

static void write_blocking(int fd, const uint8_t *p, size_t n)
{
    while (n) {
        ssize_t done = write(fd, p, n);
        assert(done > 0);
        p += done;
        n -= (size_t)done;
    }
}

static void test_duplex_pressure(void)
{
    struct app a;
    init_app(&a);
    int input[2], output[2];
    assert(pipe(input) == 0 && pipe(output) == 0);
    pid_t child = fork();
    assert(child >= 0);
    if (!child) {
        close(input[1]);
        close(output[0]);
        uint8_t pcm[PCM_PERIOD_BYTES * 8];
        uint8_t bytes[4096];
        fill_pcm(pcm, sizeof(pcm));
        /* Emit more than a pipe can hold before accepting the next chunk.
         * A synchronous parent input write cannot complete this exchange. */
        for (unsigned i = 0; i < 64; ++i) {
            write_blocking(output[1], pcm, sizeof(pcm));
            size_t got = 0;
            while (got < sizeof(bytes)) {
                ssize_t n = read(input[0], bytes + got, sizeof(bytes) - got);
                assert(n > 0);
                got += (size_t)n;
            }
            for (size_t j = 0; j < sizeof(bytes); ++j)
                assert(bytes[j] == (uint8_t)(i + j));
        }
        close(input[0]);
        close(output[1]);
        _exit(0);
    }
    close(input[0]);
    close(output[1]);
    assert(fcntl(input[1], F_SETFL, O_NONBLOCK) == 0);
    assert(fcntl(output[0], F_SETFL, O_NONBLOCK) == 0);
    a.renderer.stdin_fd = input[1];
    a.renderer.stdout_fd = output[0];
    uint8_t encoded[64 * 4096];
    for (size_t i = 0; i < sizeof(encoded); ++i)
        encoded[i] = (uint8_t)(i / 4096 + i);
    assert(queue_encoded(&a, encoded, sizeof(encoded)) == 0);
    /* alarm bounds the entire test, including a deadlocked child. */
    while (packets < 64 * 8 || a.encoded_pending_len) {
        struct pollfd fds[2] = {
            {.fd = input[1], .events = a.encoded_pending_len ? POLLOUT : 0},
            {.fd = output[0], .events = POLLIN}
        };
        assert(poll(fds, 2, 1000) > 0);
        if (fds[0].revents & POLLOUT)
            assert(flush_encoded(&a) == 0);
        if (fds[1].revents & POLLIN)
            assert(ingest_renderer_output(&a) == 0);
    }
    int status;
    assert(waitpid(child, &status, 0) == child);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 0);
    assert(a.render_pending_len == 0 && packets == 512);
    close(input[1]);
    close(output[0]);
}

int main(void)
{
    alarm(10);
    uint8_t short_packet = 0;
    assert(validate_frame(&short_packet, RX_FRAME_CAP + 1u,
                          NULL, NULL, NULL, NULL, NULL) == -1);
    test_output_retry();
    test_queue_capacity();
    test_full_output_queue();
    test_duplex_pressure();
    alarm(0);
    puts("PASS: output retry, queue bounds, output backpressure, duplex pressure (512 PCM periods)");
    return 0;
}
