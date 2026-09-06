#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHANNELS 12
#define BLOCK_FRAMES 40
#define SAMPLE_RATE 48000
#define FILTERS 4
#define DELAY_FRAMES 192

/* Aurora wire order: FL FR C LFE BL BR SL SR TFL TFR TBL TBR
   Aurora DSP order:  FL FR C LFE SL SR BL BR TFL TFR TBL TBR */
static const unsigned wire_to_dsp[CHANNELS] = {0,1,2,3,6,7,4,5,8,9,10,11};
static const unsigned dsp_to_wire[CHANNELS] = {0,1,2,3,6,7,4,5,8,9,10,11};

typedef struct {
    float z[FILTERS][CHANNELS];
    float delay[DELAY_FRAMES][CHANNELS];
    unsigned delay_pos;
} DspState;

static double now_seconds(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return 0.0;
    return (double)ts.tv_sec + (double)ts.tv_nsec * 1e-9;
}

static void wire_to_canonical(float *samples, size_t frames) {
    float tmp[CHANNELS];
    for (size_t f = 0; f < frames; ++f) {
        float *p = samples + f * CHANNELS;
        for (unsigned c = 0; c < CHANNELS; ++c) tmp[wire_to_dsp[c]] = p[c];
        memcpy(p, tmp, sizeof(tmp));
    }
}

static void canonical_to_wire(float *samples, size_t frames) {
    float tmp[CHANNELS];
    for (size_t f = 0; f < frames; ++f) {
        float *p = samples + f * CHANNELS;
        for (unsigned c = 0; c < CHANNELS; ++c) tmp[dsp_to_wire[c]] = p[c];
        memcpy(p, tmp, sizeof(tmp));
    }
}

static int test_channel_mapping(void) {
    float b[2 * CHANNELS];
    for (unsigned f = 0; f < 2; ++f)
        for (unsigned c = 0; c < CHANNELS; ++c)
            b[f * CHANNELS + c] = (float)(100 * f + c + 1);
    float original[2 * CHANNELS];
    memcpy(original, b, sizeof(b));
    wire_to_canonical(b, 2);
    if (b[4] != original[6] || b[5] != original[7] ||
        b[6] != original[4] || b[7] != original[5]) return 0;
    canonical_to_wire(b, 2);
    return memcmp(b, original, sizeof(b)) == 0;
}

static void process_block(DspState *s, float *samples, size_t frames) {
    static const float pole[FILTERS] = {0.12f, 0.23f, 0.37f, 0.51f};
    static const float gain[CHANNELS] = {
        0.96f,0.96f,0.98f,0.90f,0.94f,0.94f,0.93f,0.93f,0.92f,0.92f,0.92f,0.92f
    };
    wire_to_canonical(samples, frames);
    for (size_t f = 0; f < frames; ++f) {
        float *p = samples + f * CHANNELS;
        float bass = 0.0f;
        for (unsigned c = 0; c < CHANNELS; ++c) {
            float x = isfinite(p[c]) ? p[c] * gain[c] : 0.0f;
            for (unsigned k = 0; k < FILTERS; ++k) {
                s->z[k][c] += pole[k] * (x - s->z[k][c]);
                x = 0.78f * x + 0.22f * s->z[k][c];
            }
            float delayed = s->delay[s->delay_pos][c];
            s->delay[s->delay_pos][c] = x;
            x += ((c >= 8) ? 0.16f : 0.06f) * delayed;
            p[c] = x / (1.0f + fabsf(x));
            if (c != 3) bass += 0.012f * p[c];
        }
        p[3] = (p[3] + bass) / (1.0f + fabsf(p[3] + bass));
        s->delay_pos = (s->delay_pos + 1u) % DELAY_FRAMES;
    }
    canonical_to_wire(samples, frames);
}

static uint32_t prng_state = 0x4155524fu;
static float noise_sample(void) {
    prng_state = prng_state * 1664525u + 1013904223u;
    return ((float)((prng_state >> 8) & 0xffffu) / 32768.0f - 1.0f) * 0.3f;
}

int main(int argc, char **argv) {
    double stress_seconds = 60.0;
    if (argc == 2) {
        char *end = NULL;
        errno = 0;
        double value = strtod(argv[1], &end);
        if (errno || end == argv[1] || *end != '\0' || value < 1.0 || value > 600.0) {
            fprintf(stderr, "usage: %s [stress-seconds: 1..600]\n", argv[0]);
            return 2;
        }
        stress_seconds = value;
    }

    if (!test_channel_mapping()) {
        puts("FAIL channel_mapping");
        return 3;
    }
    puts("PASS channel_mapping_roundtrip_12ch");

    DspState state;
    memset(&state, 0, sizeof(state));
    float block[BLOCK_FRAMES * CHANNELS];
    for (size_t i = 0; i < BLOCK_FRAMES * CHANNELS; ++i) block[i] = noise_sample();

    uint64_t blocks = 0;
    double start = now_seconds();
    double current = start;
    volatile float checksum = 0.0f;
    while ((current - start) < stress_seconds) {
        process_block(&state, block, BLOCK_FRAMES);
        checksum += block[(blocks * 17u) % (BLOCK_FRAMES * CHANNELS)] * 1e-7f;
        ++blocks;
        if ((blocks & 0x3ffu) == 0) current = now_seconds();
    }
    double wall = now_seconds() - start;
    double audio_seconds = ((double)blocks * BLOCK_FRAMES) / SAMPLE_RATE;
    double realtime = wall > 0.0 ? audio_seconds / wall : 0.0;

    int finite = 1;
    for (size_t i = 0; i < BLOCK_FRAMES * CHANNELS; ++i)
        if (!isfinite(block[i])) finite = 0;
    printf("%s finite_output\n", finite ? "PASS" : "FAIL");
    printf("RESULT blocks=%llu audio_seconds=%.3f wall_seconds=%.3f realtime_factor=%.2fx checksum=%.9g\n",
           (unsigned long long)blocks, audio_seconds, wall, realtime, (double)checksum);
    printf("INTERPRETATION realtime_factor_above_1_means_this_12ch_DSP_load_kept_up_with_realtime\n");
    return finite && realtime >= 1.0 ? 0 : 4;
}
