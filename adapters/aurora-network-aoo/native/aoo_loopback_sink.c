#include <aoo.h>
#include <aoo_client.h>
#include <aoo_sink.h>

#include <math.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#define CHANNELS 12
#define SAMPLE_RATE 48000
#define BLOCK_FRAMES 48
#define SINK_ID 1

typedef struct SinkHarness {
    AooClient *client;
    AooSink *sink;
    pthread_t send_thread;
    pthread_t receive_thread;
    int threads_running;
    atomic_int stream_starts;
    atomic_int stream_stops;
} SinkHarness;

static void *send_thread_main(void *user) {
    SinkHarness *harness = (SinkHarness *)user;
    (void)AooClient_send(harness->client, kAooInfinite);
    return NULL;
}

static void *receive_thread_main(void *user) {
    SinkHarness *harness = (SinkHarness *)user;
    (void)AooClient_receive(harness->client, kAooInfinite);
    return NULL;
}

static void AOO_CALL event_handler(
    void *user,
    const AooEvent *event,
    AooThreadLevel level
) {
    (void)level;
    SinkHarness *harness = (SinkHarness *)user;
    if (event->type == kAooEventStreamStart) {
        atomic_fetch_add(&harness->stream_starts, 1);
    } else if (event->type == kAooEventStreamStop) {
        atomic_fetch_add(&harness->stream_stops, 1);
    }
}

static void stop_threads(SinkHarness *harness) {
    if (!harness->threads_running) {
        return;
    }
    (void)AooClient_stop(harness->client);
    (void)pthread_join(harness->send_thread, NULL);
    (void)pthread_join(harness->receive_thread, NULL);
    harness->threads_running = 0;
}

static int start_threads(SinkHarness *harness) {
    if (pthread_create(&harness->send_thread, NULL, send_thread_main, harness) != 0) {
        return 0;
    }
    if (pthread_create(&harness->receive_thread, NULL, receive_thread_main, harness) != 0) {
        (void)AooClient_stop(harness->client);
        (void)pthread_join(harness->send_thread, NULL);
        return 0;
    }
    harness->threads_running = 1;
    return 1;
}

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: %s PORT BLOCKS CLOCK_PPM\n", argv[0]);
        return EXIT_FAILURE;
    }
    const int port = atoi(argv[1]);
    const int blocks = atoi(argv[2]);
    const double clock_ppm = strtod(argv[3], NULL);
    if (port <= 0 || port > 65535 || blocks <= 0 || !isfinite(clock_ppm)) {
        fprintf(stderr, "invalid loopback sink arguments\n");
        return EXIT_FAILURE;
    }

    AooSettings settings = AOO_SETTINGS_INIT();
    if (aoo_initialize(&settings) != kAooOk) {
        fprintf(stderr, "aoo_initialize failed\n");
        return EXIT_FAILURE;
    }

    SinkHarness harness = {0};
    atomic_init(&harness.stream_starts, 0);
    atomic_init(&harness.stream_stops, 0);
    harness.sink = AooSink_new(SINK_ID);
    harness.client = AooClient_new();
    if (harness.sink == NULL || harness.client == NULL) {
        fprintf(stderr, "AOO sink/client allocation failed\n");
        goto fail;
    }

    if (AooSink_setEventHandler(
            harness.sink,
            event_handler,
            &harness,
            kAooEventModeCallback
        ) != kAooOk) {
        fprintf(stderr, "AooSink_setEventHandler failed\n");
        goto fail;
    }
    if (AooSink_setLatency(harness.sink, 0.08) != kAooOk) {
        fprintf(stderr, "AooSink_setLatency failed\n");
        goto fail;
    }
    if (AooSink_setDynamicResampling(harness.sink, kAooTrue) != kAooOk) {
        fprintf(stderr, "AooSink_setDynamicResampling failed\n");
        goto fail;
    }

    AooClientSettings client_settings = AOO_CLIENT_SETTINGS_INIT();
    client_settings.portNumber = (AooUInt16)port;
    if (AooClient_setup(harness.client, &client_settings) != kAooOk) {
        fprintf(stderr, "AooClient_setup failed\n");
        goto fail;
    }
    if (AooSink_setup(
            harness.sink,
            CHANNELS,
            SAMPLE_RATE,
            BLOCK_FRAMES,
            0
        ) != kAooOk) {
        fprintf(stderr, "AooSink_setup failed\n");
        goto fail;
    }
    if (AooClient_addSink(harness.client, harness.sink) != kAooOk) {
        fprintf(stderr, "AooClient_addSink failed\n");
        goto fail;
    }
    if (!start_threads(&harness)) {
        fprintf(stderr, "network thread startup failed\n");
        goto fail;
    }

    AooSample *channels[CHANNELS] = {0};
    AooSample *storage = (AooSample *)calloc(
        (size_t)CHANNELS * (size_t)BLOCK_FRAMES,
        sizeof(AooSample)
    );
    if (storage == NULL) {
        fprintf(stderr, "output buffer allocation failed\n");
        goto fail;
    }
    for (int channel = 0; channel < CHANNELS; ++channel) {
        channels[channel] = storage + (size_t)channel * (size_t)BLOCK_FRAMES;
    }

    const AooNtpTime base_ntp = aoo_getCurrentNtpTime();
    const AooSeconds base_seconds = aoo_ntpTimeToSeconds(base_ntp);
    const double clock_scale = 1.0 + clock_ppm / 1000000.0;
    const struct timespec sleep_time = {0, 1000000};
    int active_blocks = 0;
    int non_finite_blocks = 0;
    double total_energy = 0.0;

    for (int block = 0; block < blocks; ++block) {
        const double nominal_seconds =
            (double)block * (double)BLOCK_FRAMES / (double)SAMPLE_RATE;
        const AooNtpTime process_time = aoo_ntpTimeFromSeconds(
            base_seconds + nominal_seconds * clock_scale
        );
        const AooError error = AooSink_process(
            harness.sink,
            channels,
            BLOCK_FRAMES,
            process_time,
            NULL,
            NULL
        );
        if (error != kAooOk && error != kAooErrorIdle) {
            fprintf(stderr, "AooSink_process failed with %d\n", (int)error);
            free(storage);
            goto fail;
        }
        (void)AooClient_notify(harness.client);

        double block_energy = 0.0;
        int finite = 1;
        for (int channel = 0; channel < CHANNELS; ++channel) {
            for (int frame = 0; frame < BLOCK_FRAMES; ++frame) {
                const double sample = (double)channels[channel][frame];
                if (!isfinite(sample)) {
                    finite = 0;
                }
                block_energy += fabs(sample);
            }
        }
        if (!finite) {
            non_finite_blocks += 1;
        }
        if (block_energy > 0.000001) {
            active_blocks += 1;
        }
        total_energy += block_energy;
        (void)nanosleep(&sleep_time, NULL);
    }

    AooSampleRate real_sample_rate = 0.0;
    (void)AooSink_getRealSampleRate(harness.sink, &real_sample_rate);
    const int starts = atomic_load(&harness.stream_starts);
    const int stops = atomic_load(&harness.stream_stops);
    const int minimum_active = blocks / 5;

    free(storage);
    stop_threads(&harness);
    AooSink_free(harness.sink);
    AooClient_free(harness.client);
    aoo_terminate();

    if (starts < 1 || active_blocks < minimum_active || non_finite_blocks != 0 ||
        !isfinite(total_energy) || total_energy <= 0.0) {
        fprintf(
            stderr,
            "loopback acceptance failed starts=%d active=%d/%d nonfinite=%d energy=%.9f ppm=%.3f real_sr=%.3f\n",
            starts,
            active_blocks,
            blocks,
            non_finite_blocks,
            total_energy,
            clock_ppm,
            (double)real_sample_rate
        );
        return EXIT_FAILURE;
    }

    printf(
        "aurora-aoo-loopback-sink: PASS blocks=%d active=%d starts=%d stops=%d ppm=%.3f real_sr=%.3f energy=%.9f\n",
        blocks,
        active_blocks,
        starts,
        stops,
        clock_ppm,
        (double)real_sample_rate,
        total_energy
    );
    return EXIT_SUCCESS;

fail:
    stop_threads(&harness);
    if (harness.sink != NULL) {
        AooSink_free(harness.sink);
    }
    if (harness.client != NULL) {
        AooClient_free(harness.client);
    }
    aoo_terminate();
    return EXIT_FAILURE;
}
