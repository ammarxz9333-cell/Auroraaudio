#include "iec61937_eac3.h"

#include <assert.h>
#include <stdio.h>
#include <string.h>

struct sink {
    uint8_t data[256];
    size_t len;
    unsigned calls;
};

static int emit(void *opaque, const uint8_t *payload, size_t len)
{
    struct sink *s = opaque;
    assert(s->len + len <= sizeof(s->data));
    memcpy(s->data + s->len, payload, len);
    s->len += len;
    s->calls++;
    return 0;
}

static void make_eac3_burst(uint8_t *out, const uint8_t *payload, size_t payload_len)
{
    assert((payload_len & 1u) == 0);
    out[0] = 0x72;
    out[1] = 0xf8;
    out[2] = 0x1f;
    out[3] = 0x4e;
    out[4] = 0x15;
    out[5] = 0x00;
    out[6] = (uint8_t)(payload_len & 0xffu);
    out[7] = (uint8_t)((payload_len >> 8) & 0xffu);
    for (size_t i = 0; i < payload_len; i += 2) {
        out[8 + i] = payload[i + 1];
        out[8 + i + 1] = payload[i];
    }
}

int main(void)
{
    struct aurora_iec61937_eac3_parser p;
    struct sink s;
    uint8_t burst[64];
    const uint8_t eac3[] = {0x0b, 0x77, 0x12, 0x34, 0x56, 0x78};

    memset(&s, 0, sizeof(s));
    aurora_iec61937_eac3_init(&p);
    make_eac3_burst(burst, eac3, sizeof(eac3));

    /* Split across arbitrary USB/application boundaries. */
    assert(aurora_iec61937_eac3_feed(&p, burst, 3, emit, &s) == 0);
    assert(s.calls == 0);
    assert(aurora_iec61937_eac3_feed(&p, burst + 3, 4, emit, &s) == 0);
    assert(s.calls == 0);
    assert(aurora_iec61937_eac3_feed(&p, burst + 7, 7, emit, &s) == 0);
    assert(s.calls == 1);
    assert(s.len == sizeof(eac3));
    assert(memcmp(s.data, eac3, sizeof(eac3)) == 0);

    /* Two coalesced bursts plus padding/noise must both emerge. */
    memset(&s, 0, sizeof(s));
    aurora_iec61937_eac3_reset(&p);
    uint8_t combo[128];
    size_t off = 0;
    combo[off++] = 0xaa;
    combo[off++] = 0xbb;
    memcpy(combo + off, burst, 8 + sizeof(eac3));
    off += 8 + sizeof(eac3);
    memset(combo + off, 0, 11);
    off += 11;
    memcpy(combo + off, burst, 8 + sizeof(eac3));
    off += 8 + sizeof(eac3);
    assert(aurora_iec61937_eac3_feed(&p, combo, off, emit, &s) == 0);
    assert(s.calls == 2);
    assert(s.len == 2 * sizeof(eac3));
    assert(memcmp(s.data, eac3, sizeof(eac3)) == 0);
    assert(memcmp(s.data + sizeof(eac3), eac3, sizeof(eac3)) == 0);

    /* Non-EAC3 type is ignored, then parser recovers at the next real preamble. */
    memset(&s, 0, sizeof(s));
    aurora_iec61937_eac3_reset(&p);
    uint8_t mixed[128];
    memcpy(mixed, burst, 8 + sizeof(eac3));
    mixed[4] = 0x01;
    memcpy(mixed + 8 + sizeof(eac3), burst, 8 + sizeof(eac3));
    assert(aurora_iec61937_eac3_feed(&p, mixed,
                                     2 * (8 + sizeof(eac3)), emit, &s) == 0);
    assert(s.calls == 1);
    assert(memcmp(s.data, eac3, sizeof(eac3)) == 0);

    puts("IEC61937 E-AC-3 streaming parser tests passed");
    return 0;
}
