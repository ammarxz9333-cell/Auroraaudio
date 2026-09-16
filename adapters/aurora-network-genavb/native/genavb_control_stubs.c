#include <stdint.h>
#include <genavb/genavb.h>

int genavb_control_open(const struct genavb_handle *genavb, struct genavb_control_handle **handle,
                        genavb_control_id_t id)
{
    (void)genavb;
    (void)id;
    *handle = (struct genavb_control_handle *)(uintptr_t)0x2000u;
    return GENAVB_SUCCESS;
}

int genavb_control_close(struct genavb_control_handle *handle)
{
    (void)handle;
    return GENAVB_SUCCESS;
}

int genavb_control_rx_fd(const struct genavb_control_handle *handle)
{
    (void)handle;
    return 42;
}

int genavb_control_receive(const struct genavb_control_handle *handle, genavb_msg_type_t *msg_type,
                           void *msg, unsigned int *msg_len)
{
    (void)handle;
    (void)msg_type;
    (void)msg;
    (void)msg_len;
    return -1;
}
