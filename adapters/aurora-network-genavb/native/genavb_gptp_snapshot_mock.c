#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <genavb/control.h>
#include <genavb/control_clock_domain.h>
#include <genavb/control_gptp.h>
#include <genavb/genavb.h>

static const char *scenario(void)
{
    const char *value = getenv("AURORA_GPTP_MOCK_SCENARIO");
    return value && value[0] ? value : "locked";
}

int genavb_init(struct genavb_handle **genavb, unsigned int flags)
{
    (void)flags;
    *genavb = (struct genavb_handle *)(uintptr_t)0x1000u;
    return GENAVB_SUCCESS;
}

int genavb_exit(struct genavb_handle *genavb)
{
    (void)genavb;
    return GENAVB_SUCCESS;
}

int genavb_control_open(const struct genavb_handle *genavb,
                        struct genavb_control_handle **handle,
                        genavb_control_id_t id)
{
    (void)genavb;
    *handle = (struct genavb_control_handle *)(uintptr_t)(0x2000u + (unsigned int)id);
    return GENAVB_SUCCESS;
}

int genavb_control_close(struct genavb_control_handle *handle)
{
    (void)handle;
    return GENAVB_SUCCESS;
}

int genavb_control_send_sync(const struct genavb_control_handle *handle,
                             genavb_msg_type_t *msg_type,
                             const void *msg,
                             unsigned int msg_len,
                             void *rsp,
                             unsigned int *rsp_len,
                             int timeout)
{
    const char *mode = scenario();

    (void)handle;
    (void)msg_len;
    (void)timeout;

    if (*msg_type == GENAVB_MSG_GM_GET_STATUS) {
        static const uint8_t identity[8] = {0x02, 0x00, 0x00, 0xff, 0xfe, 0x00, 0x00, 0x01};
        const struct genavb_msg_gm_get_status *request = msg;
        struct genavb_msg_gm_status *response = rsp;

        if (*rsp_len < sizeof(*response))
            return -1;
        memset(response, 0, sizeof(*response));
        response->domain = request->domain;
        if (strcmp(mode, "gm-zero") != 0)
            memcpy(&response->gm_id, identity, sizeof(identity));
        response->num_ptlv = 0;
        *rsp_len = sizeof(*response);
        *msg_type = GENAVB_MSG_GM_STATUS;
        return GENAVB_SUCCESS;
    }

    if (*msg_type == GENAVB_MSG_CLOCK_DOMAIN_GET_STATUS) {
        const struct genavb_msg_clock_domain_get_status *request = msg;
        struct genavb_msg_clock_domain_status *response = rsp;

        if (*rsp_len < sizeof(*response))
            return -1;
        memset(response, 0, sizeof(*response));
        response->domain = request->domain;
        response->source_type = GENAVB_CLOCK_SOURCE_TYPE_INTERNAL;
        response->local_id = GENAVB_CLOCK_SOURCE_PTP_CLK;
        response->status = GENAVB_CLOCK_DOMAIN_STATUS_LOCKED;

        if (strcmp(mode, "unlocked") == 0)
            response->status = GENAVB_CLOCK_DOMAIN_STATUS_UNLOCKED;
        else if (strcmp(mode, "free-wheeling") == 0)
            response->status = GENAVB_CLOCK_DOMAIN_STATUS_FREE_WHEELING;
        else if (strcmp(mode, "wrong-source") == 0)
            response->local_id = GENAVB_CLOCK_SOURCE_AUDIO_CLK;

        *rsp_len = sizeof(*response);
        *msg_type = GENAVB_MSG_CLOCK_DOMAIN_STATUS;
        return GENAVB_SUCCESS;
    }

    return -1;
}
