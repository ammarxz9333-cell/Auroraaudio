#include "iec61937_eac3.h"

#include <string.h>

static const uint8_t pa_le[2] = {0x72, 0xf8};
static const uint8_t pb_le[2] = {0x1f, 0x4e};

static uint16_t u16le(const uint8_t *p)
{
    return (uint16_t)p[0] | ((uint16_t)p[1] << 8);
}

void aurora_iec61937_eac3_init(struct aurora_iec61937_eac3_parser *p)
{
    if (p)
        memset(p, 0, sizeof(*p));
}

void aurora_iec61937_eac3_reset(struct aurora_iec61937_eac3_parser *p)
{
    if (!p)
        return;
    p->len = 0;
}

static size_t find_pa(const uint8_t *buf, size_t len, size_t start)
{
    if (len < 2 || start >= len - 1)
        return SIZE_MAX;
    for (size_t i = start; i + 1 < len; ++i) {
        if (buf[i] == pa_le[0] && buf[i + 1] == pa_le[1])
            return i;
    }
    return SIZE_MAX;
}

static void compact(struct aurora_iec61937_eac3_parser *p, size_t consumed)
{
    if (consumed == 0)
        return;
    if (consumed >= p->len) {
        p->len = 0;
        return;
    }
    memmove(p->buffer, p->buffer + consumed, p->len - consumed);
    p->len -= consumed;
}

int aurora_iec61937_eac3_feed(struct aurora_iec61937_eac3_parser *p,
                              const uint8_t *data, size_t len,
                              aurora_eac3_emit_fn emit, void *opaque)
{
    if (!p || (!data && len) || !emit)
        return -1;

    if (len > AURORA_IEC61937_BUFFER_CAPACITY - p->len) {
        size_t keep = p->len < 3 ? p->len : 3;
        if (keep)
            memmove(p->buffer, p->buffer + p->len - keep, keep);
        p->resync_bytes += p->len - keep;
        p->len = keep;
        if (len > AURORA_IEC61937_BUFFER_CAPACITY - p->len)
            return -2;
    }

    if (len) {
        memcpy(p->buffer + p->len, data, len);
        p->len += len;
    }

    size_t scan = 0;
    size_t consumed = 0;

    while (1) {
        size_t j = find_pa(p->buffer, p->len, scan);
        if (j == SIZE_MAX) {
            size_t keep = p->len < 3 ? p->len : 3;
            consumed = p->len - keep;
            p->resync_bytes += consumed;
            break;
        }

        if (j > scan)
            p->resync_bytes += j - scan;

        if (j + 8 > p->len) {
            consumed = j;
            break;
        }

        if (p->buffer[j + 2] != pb_le[0] || p->buffer[j + 3] != pb_le[1]) {
            scan = j + 2;
            continue;
        }

        uint16_t pc = u16le(p->buffer + j + 4);
        uint16_t pd = u16le(p->buffer + j + 6);
        uint8_t data_type = (uint8_t)(pc & 0x1fu);

        if (data_type != AURORA_IEC61937_EAC3_TYPE) {
            p->rejected_bursts++;
            scan = j + 2;
            continue;
        }

        /* IEC61937 data type 0x15 (E-AC-3) expresses Pd in bytes. */
        size_t payload_bytes = (size_t)pd;
        if (payload_bytes == 0 || (payload_bytes & 1u) != 0 ||
            payload_bytes > AURORA_IEC61937_BUFFER_CAPACITY - 8u) {
            p->rejected_bursts++;
            scan = j + 2;
            continue;
        }

        if (j + 8u + payload_bytes > p->len) {
            consumed = j;
            break;
        }

        /* Canonical Aurora carrier is S16_LE word order. Convert back to native E-AC-3 bytes. */
        uint8_t *payload = p->buffer + j + 8u;
        for (size_t k = 0; k < payload_bytes; k += 2u) {
            uint8_t tmp = payload[k];
            payload[k] = payload[k + 1u];
            payload[k + 1u] = tmp;
        }

        int rc = emit(opaque, payload, payload_bytes);

        /* Restore buffered carrier representation before compacting. */
        for (size_t k = 0; k < payload_bytes; k += 2u) {
            uint8_t tmp = payload[k];
            payload[k] = payload[k + 1u];
            payload[k + 1u] = tmp;
        }

        if (rc != 0)
            return -3;

        p->emitted_bursts++;
        consumed = j + 8u + payload_bytes;
        scan = consumed;

        if (scan >= p->len)
            break;
    }

    compact(p, consumed);
    return 0;
}
