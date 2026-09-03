#ifndef AURORA_IEC61937_EAC3_H
#define AURORA_IEC61937_EAC3_H

#include <stddef.h>
#include <stdint.h>

#define AURORA_IEC61937_BUFFER_CAPACITY (256u * 1024u)
#define AURORA_IEC61937_EAC3_TYPE 0x15u

struct aurora_iec61937_eac3_parser {
    uint8_t buffer[AURORA_IEC61937_BUFFER_CAPACITY];
    size_t len;
    uint64_t emitted_bursts;
    uint64_t rejected_bursts;
    uint64_t resync_bytes;
};

typedef int (*aurora_eac3_emit_fn)(void *opaque, const uint8_t *payload, size_t len);

void aurora_iec61937_eac3_init(struct aurora_iec61937_eac3_parser *p);
void aurora_iec61937_eac3_reset(struct aurora_iec61937_eac3_parser *p);
int aurora_iec61937_eac3_feed(struct aurora_iec61937_eac3_parser *p,
                              const uint8_t *data, size_t len,
                              aurora_eac3_emit_fn emit, void *opaque);

#endif
