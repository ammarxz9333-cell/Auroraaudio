#include <aoo.h>
#include <aoo_client.h>
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

static void cleanup(AooClient *client, AooSource *source, int source_registered) {
    if (client != NULL && source != NULL && source_registered) {
        AooClient_removeSource(client, source);
    }
    if (client != NULL) {
        AooClient_free(client);
    }
    if (source != NULL) {
        AooSource_free(source);
    }
    aoo_terminate();
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

    AooClient *client = AooClient_new();
    if (client == NULL) {
        fprintf(stderr, "AooClient_new returned NULL\n");
        cleanup(NULL, source, 0);
        return EXIT_FAILURE;
    }

    AooClientSettings client_settings = AOO_CLIENT_SETTINGS_INIT();
    client_settings.portNumber = 0;
    error = AooClient_setup(client, &client_settings);
    if (error != kAooOk) {
        cleanup(client, source, 0);
        return fail("AooClient_setup", error);
    }

    error = AooSource_setup(
        source,
        AURORA_CHANNELS,
        AURORA_SAMPLE_RATE,
        AURORA_BLOCK_FRAMES,
        0
    );
    if (error != kAooOk) {
        cleanup(client, source, 0);
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
        cleanup(client, source, 0);
        return fail("AooSource_setFormat", error);
    }

    error = AooClient_addSource(client, source);
    if (error != kAooOk) {
        cleanup(client, source, 0);
        return fail("AooClient_addSource", error);
    }
    const int source_registered = 1;

    error = AooSource_startStream(source, 0, NULL);
    if (error != kAooOk) {
        cleanup(client, source, source_registered);
        return fail("AooSource_startStream", error);
    }

    AooSample planar[AURORA_CHANNELS][AURORA_BLOCK_FRAMES];
    AooSample *channels[AURORA_CHANNELS];
    for (int channel = 0; channel < AURORA_CHANNELS; ++channel) {
        channels[channel] = planar[channel];
        for (int frame = 0; frame < AURORA_BLOCK_FRAMES; ++frame) {
            planar[channel][frame] = (AooSample)((channel + 1) * 0.001 + frame * 0.00001);
        }
    }

    const AooNtpTime first_ntp = aoo_getCurrentNtpTime();
    const AooSeconds block_seconds =
        (AooSeconds)AURORA_BLOCK_FRAMES / (AooSeconds)AURORA_SAMPLE_RATE;
    const AooNtpTime second_ntp =
        aoo_ntpTimeFromSeconds(aoo_ntpTimeToSeconds(first_ntp) + block_seconds);
    const AooSeconds mapped_duration = aoo_ntpTimeDuration(first_ntp, second_ntp);
    if (mapped_duration < block_seconds - 0.000001 ||
        mapped_duration > block_seconds + 0.000001) {
        fprintf(stderr, "Aurora media-frame to AOO NTP mapping drifted: %.9f\n", mapped_duration);
        AooSource_stopStream(source, 0);
        cleanup(client, source, source_registered);
        return EXIT_FAILURE;
    }

    error = AooSource_process(source, channels, AURORA_BLOCK_FRAMES, first_ntp);
    if (error != kAooOk) {
        AooSource_stopStream(source, 0);
        cleanup(client, source, source_registered);
        return fail("AooSource_process(first)", error);
    }
    error = AooSource_process(source, channels, AURORA_BLOCK_FRAMES, second_ntp);
    if (error != kAooOk) {
        AooSource_stopStream(source, 0);
        cleanup(client, source, source_registered);
        return fail("AooSource_process(second)", error);
    }

    error = AooSource_stopStream(source, 0);
    if (error != kAooOk) {
        cleanup(client, source, source_registered);
        return fail("AooSource_stopStream", error);
    }

    printf(
        "aurora-aoo-contract: PASS channels=%d rate=%d block=%d codec=pcm-f32 lifecycle=client-source-process timestamp-map=ntp\n",
        AURORA_CHANNELS,
        AURORA_SAMPLE_RATE,
        AURORA_BLOCK_FRAMES
    );

    cleanup(client, source, source_registered);
    return EXIT_SUCCESS;
}
