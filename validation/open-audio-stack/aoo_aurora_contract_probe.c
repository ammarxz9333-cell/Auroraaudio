#include <aoo.h>
#include <aoo_source.h>
#include <codec/aoo_pcm.h>

#include <stdio.h>
#include <stdlib.h>

#define AURORA_CHANNELS 12
#define AURORA_SAMPLE_RATE 48000
#define AURORA_BLOCK_FRAMES 48

static int fail(const char *message, AooError error) {
    fprintf(stderr, "%s: %s\n", message, aoo_strerror(error));
    return EXIT_FAILURE;
}

int main(void) {
    AooSettings settings = AOO_SETTINGS_INIT();
    AooError error = aoo_initialize(&settings);
    if (error != kAooOk) {
        return fail("aoo_initialize", error);
    }

    AooSource *source = AooSource_new(1);
    if (source == NULL) {
        fprintf(stderr, "AooSource_new returned NULL\n");
        aoo_terminate();
        return EXIT_FAILURE;
    }

    error = AooSource_setup(
        source,
        AURORA_CHANNELS,
        AURORA_SAMPLE_RATE,
        AURORA_BLOCK_FRAMES,
        0
    );
    if (error != kAooOk) {
        AooSource_free(source);
        aoo_terminate();
        return fail("AooSource_setup", error);
    }

    AooFormatPcm format;
    AooFormatPcm_init(
        &format,
        AURORA_CHANNELS,
        AURORA_SAMPLE_RATE,
        AURORA_BLOCK_FRAMES,
        kAooPcmFloat32
    );
    error = AooSource_setFormat(source, &format.header);
    if (error != kAooOk) {
        AooSource_free(source);
        aoo_terminate();
        return fail("AooSource_setFormat", error);
    }

    printf(
        "aurora-aoo-contract: PASS channels=%d rate=%d block=%d codec=pcm-f32\n",
        AURORA_CHANNELS,
        AURORA_SAMPLE_RATE,
        AURORA_BLOCK_FRAMES
    );

    AooSource_free(source);
    aoo_terminate();
    return EXIT_SUCCESS;
}
