#include <stddef.h>
#include <stdint.h>
#include <string.h>

typedef struct {
    uint32_t sample_rate;
    const char *config_yaml_path;
    const char *speaker_layout_path;
    const char *bridge_path;
    const char *codec;
    int osc_enabled;
    uint16_t osc_port_in;
    uint16_t osc_port_out;
    const char *osc_bind;
    const char *osc_host;
} OrenderConfig;

static int renderer_token;
static const uint8_t layout[12] = {0, 1, 2, 3, 14, 15, 4, 5, 6, 7, 10, 11};

uint32_t orender_version_major(void) { return 0; }
uint32_t orender_version_minor(void) { return 7; }
const char *orender_build_id(void) { return "aurora-ci-liborender-stub"; }

void *orender_create(const OrenderConfig *cfg) {
    if (!cfg || cfg->sample_rate != 48000 || !cfg->speaker_layout_path ||
        !cfg->bridge_path || !cfg->codec || strcmp(cfg->codec, "eac3") != 0 ||
        cfg->osc_enabled != 0) {
        return NULL;
    }
    return &renderer_token;
}

void orender_destroy(void *renderer) { (void)renderer; }
uint32_t orender_channel_count(const void *renderer) {
    return renderer ? 12 : 0;
}
uint32_t orender_channel_layout(const void *renderer, uint8_t *out, uint32_t capacity) {
    if (!renderer || !out || capacity < 12) return 0;
    memcpy(out, layout, sizeof(layout));
    return 12;
}
void orender_set_channel_mapping(void *renderer, int mapping) {
    (void)renderer; (void)mapping;
}
void orender_set_channel_mode(void *renderer, int mode) {
    (void)renderer; (void)mode;
}

int orender_process(void *renderer,
                    const uint8_t *packet,
                    size_t packet_len,
                    int64_t pts,
                    float *out_interleaved,
                    size_t out_capacity_samples,
                    size_t *out_frames,
                    uint32_t *out_channels,
                    int64_t *out_pts) {
    (void)pts;
    if (!renderer || !packet || packet_len == 0 || !out_frames || !out_channels || !out_pts)
        return -1;
    if (!out_interleaved || out_capacity_samples < 12) return 1;
    for (size_t i = 0; i < 12; ++i) out_interleaved[i] = (float)i / 16.0f;
    *out_frames = 1;
    *out_channels = 12;
    *out_pts = 1234;
    return 0;
}

uint64_t orender_output_latency_samples(const void *renderer) {
    return renderer ? 64 : 0;
}
int orender_has_objects(const void *renderer) { return renderer ? 1 : 0; }
int orender_object_count(const void *renderer) { return renderer ? 15 : 0; }
