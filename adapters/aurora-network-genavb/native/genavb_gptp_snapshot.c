#define _POSIX_C_SOURCE 200809L

#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include <genavb/control.h>
#include <genavb/control_clock_domain.h>
#include <genavb/control_gptp.h>
#include <genavb/genavb.h>

#define SNAPSHOT_SCHEMA "aurora.genavb.nxp-gptp-snapshot.v1"
#define RESPONSE_BYTES 1024u
#define CONTROL_TIMEOUT_MS 1000

static uint64_t unix_ms(void)
{
    struct timespec ts;

    if (clock_gettime(CLOCK_REALTIME, &ts) != 0)
        return 0;

    return (uint64_t)ts.tv_sec * 1000ULL + (uint64_t)ts.tv_nsec / 1000000ULL;
}

static int get_gm_status(struct genavb_handle *genavb, uint8_t domain, uint64_t *gm_id)
{
    struct genavb_control_handle *control = NULL;
    struct genavb_msg_gm_get_status request;
    uint8_t response[RESPONSE_BYTES];
    struct genavb_msg_gm_status *status = (struct genavb_msg_gm_status *)response;
    genavb_msg_type_t type = GENAVB_MSG_GM_GET_STATUS;
    unsigned int response_len = sizeof(response);
    int rc;

    memset(&request, 0, sizeof(request));
    memset(response, 0, sizeof(response));
    request.domain = domain;

    rc = genavb_control_open(genavb, &control, GENAVB_CTRL_GPTP);
    if (rc != GENAVB_SUCCESS)
        return rc;

    rc = genavb_control_send_sync(control, &type, &request, sizeof(request), response,
                                  &response_len, CONTROL_TIMEOUT_MS);
    if (rc != GENAVB_SUCCESS)
        goto out;
    if (type != GENAVB_MSG_GM_STATUS || response_len < sizeof(*status) || status->domain != domain) {
        rc = -1;
        goto out;
    }

    *gm_id = status->gm_id;

out:
    genavb_control_close(control);
    return rc;
}

static int get_clock_domain_status(struct genavb_handle *genavb,
                                   genavb_clock_domain_t domain,
                                   struct genavb_msg_clock_domain_status *status)
{
    struct genavb_control_handle *control = NULL;
    struct genavb_msg_clock_domain_get_status request;
    genavb_msg_type_t type = GENAVB_MSG_CLOCK_DOMAIN_GET_STATUS;
    unsigned int response_len = sizeof(*status);
    int rc;

    memset(&request, 0, sizeof(request));
    memset(status, 0, sizeof(*status));
    request.domain = domain;

    rc = genavb_control_open(genavb, &control, GENAVB_CTRL_CLOCK_DOMAIN);
    if (rc != GENAVB_SUCCESS)
        return rc;

    rc = genavb_control_send_sync(control, &type, &request, sizeof(request), status,
                                  &response_len, CONTROL_TIMEOUT_MS);
    if (rc != GENAVB_SUCCESS)
        goto out;
    if (type != GENAVB_MSG_CLOCK_DOMAIN_STATUS || response_len < sizeof(*status) ||
        status->domain != domain) {
        rc = -1;
        goto out;
    }

out:
    genavb_control_close(control);
    return rc;
}

static const char *clock_status_name(genavb_clock_domain_status_t status)
{
    switch (status) {
    case GENAVB_CLOCK_DOMAIN_STATUS_UNLOCKED:
        return "UNLOCKED";
    case GENAVB_CLOCK_DOMAIN_STATUS_LOCKED:
        return "LOCKED";
    case GENAVB_CLOCK_DOMAIN_STATUS_FREE_WHEELING:
        return "FREE_WHEELING";
    case GENAVB_CLOCK_DOMAIN_STATUS_HW_ERROR:
        return "HW_ERROR";
    default:
        return "UNKNOWN";
    }
}

static const char *source_type_name(genavb_clock_source_type_t source_type)
{
    switch (source_type) {
    case GENAVB_CLOCK_SOURCE_TYPE_INTERNAL:
        return "INTERNAL";
    case GENAVB_CLOCK_SOURCE_TYPE_INPUT_STREAM:
        return "INPUT_STREAM";
    case GENAVB_CLOCK_SOURCE_TYPE_INPUT_SET:
        return "INPUT_SET";
    default:
        return "UNKNOWN";
    }
}

static int parse_u8(const char *value, uint8_t *out)
{
    char *end = NULL;
    unsigned long parsed;

    parsed = strtoul(value, &end, 0);
    if (!value[0] || !end || *end || parsed > UINT8_MAX)
        return -1;
    *out = (uint8_t)parsed;
    return 0;
}

static int parse_clock_domain(const char *value, genavb_clock_domain_t *out)
{
    char *end = NULL;
    long parsed;

    parsed = strtol(value, &end, 0);
    if (!value[0] || !end || *end || parsed < GENAVB_CLOCK_DOMAIN_0 ||
        parsed >= GENAVB_CLOCK_DOMAIN_MAX)
        return -1;
    *out = (genavb_clock_domain_t)parsed;
    return 0;
}

static void print_gm_id(uint64_t gm_id)
{
    unsigned int shift;

    for (shift = 56; shift <= 56; shift -= 8) {
        printf("%02" PRIx64, (gm_id >> shift) & 0xffULL);
        if (shift)
            putchar(':');
        if (shift == 0)
            break;
    }
}

int main(int argc, char **argv)
{
    struct genavb_handle *genavb = NULL;
    struct genavb_msg_clock_domain_status clock_status;
    genavb_clock_domain_t clock_domain = GENAVB_CLOCK_DOMAIN_0;
    uint8_t gptp_domain = 0;
    uint64_t gm_id = 0;
    uint64_t sampled_at;
    int locked;
    int rc;

    if (argc > 3) {
        fprintf(stderr, "usage: %s [gptp-domain=0] [clock-domain=%d]\n", argv[0],
                GENAVB_CLOCK_DOMAIN_0);
        return 2;
    }
    if (argc >= 2 && parse_u8(argv[1], &gptp_domain) < 0) {
        fprintf(stderr, "invalid gPTP domain: %s\n", argv[1]);
        return 2;
    }
    if (argc >= 3 && parse_clock_domain(argv[2], &clock_domain) < 0) {
        fprintf(stderr, "invalid clock domain: %s\n", argv[2]);
        return 2;
    }

    sampled_at = unix_ms();
    if (!sampled_at) {
        fprintf(stderr, "failed to read CLOCK_REALTIME\n");
        return 1;
    }

    rc = genavb_init(&genavb, 0);
    if (rc != GENAVB_SUCCESS || !genavb) {
        fprintf(stderr, "genavb_init failed: %d\n", rc);
        return 1;
    }

    rc = get_gm_status(genavb, gptp_domain, &gm_id);
    if (rc != GENAVB_SUCCESS) {
        fprintf(stderr, "GM status query failed: %d\n", rc);
        genavb_exit(genavb);
        return 1;
    }

    rc = get_clock_domain_status(genavb, clock_domain, &clock_status);
    if (rc != GENAVB_SUCCESS) {
        fprintf(stderr, "clock-domain status query failed: %d\n", rc);
        genavb_exit(genavb);
        return 1;
    }

    genavb_exit(genavb);

    locked = gm_id != 0 &&
             clock_status.status == GENAVB_CLOCK_DOMAIN_STATUS_LOCKED &&
             clock_status.source_type == GENAVB_CLOCK_SOURCE_TYPE_INTERNAL &&
             clock_status.local_id == GENAVB_CLOCK_SOURCE_PTP_CLK;

    printf("{\"schema\":\"%s\",\"verdict\":\"%s\",\"sample_unix_ms\":%" PRIu64
           ",\"gptp_domain\":%u,\"clock_domain\":%d,\"grandmaster_id\":\"",
           SNAPSHOT_SCHEMA, locked ? "PASS" : "FAIL", sampled_at,
           (unsigned int)gptp_domain, (int)clock_domain);
    print_gm_id(gm_id);
    printf("\",\"clock_status\":\"%s\",\"source_type\":\"%s\",\"source_local_id\":%d,"
           "\"locked\":%s}\n",
           clock_status_name(clock_status.status), source_type_name(clock_status.source_type),
           clock_status.source_type == GENAVB_CLOCK_SOURCE_TYPE_INTERNAL ? (int)clock_status.local_id : -1,
           locked ? "true" : "false");

    return locked ? 0 : 1;
}
