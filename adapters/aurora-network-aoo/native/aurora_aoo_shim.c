#include <aoo.h>
#include <aoo_client.h>
#include <aoo_source.h>
#include <codec/aoo_pcm.h>

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#define AURORA_SHIM_EXPORT __declspec(dllexport)
#else
#include <pthread.h>
#define AURORA_SHIM_EXPORT __attribute__((visibility("default")))
#endif

#define AURORA_AOO_ABI_VERSION 1u
#define AURORA_AOO_ERR_ARGUMENT -1001
#define AURORA_AOO_ERR_STATE -1002
#define AURORA_AOO_ERR_ALLOC -1003
#define AURORA_AOO_ERR_THREAD -1004
#define AURORA_AOO_ERR_TIMELINE -1005

typedef struct AuroraAooShimConfig {
    int32_t channels;
    int32_t sample_rate;
    int32_t block_frames;
    int32_t source_id;
    int32_t local_port;
    int32_t sink_port;
    int32_t sink_id;
    const char *sink_host;
} AuroraAooShimConfig;

typedef struct AuroraAooHandle {
    AooClient *client;
    AooSource *source;
    int source_registered;
    int prepared;
    int started;
    int network_threads_running;
    int channels;
    int sample_rate;
    int block_frames;
    AooSample **planar_channels;
    AooSample *planar_storage;
    AooSockAddrStorage sink_address;
    AooEndpoint sink_endpoint;
    int sink_added;
    int has_time_base;
    uint64_t base_media_frame;
    AooNtpTime base_ntp;
#ifdef _WIN32
    HANDLE send_thread;
    HANDLE receive_thread;
#else
    pthread_t send_thread;
    pthread_t receive_thread;
#endif
} AuroraAooHandle;

#ifdef _WIN32
static DWORD WINAPI aurora_aoo_send_thread(LPVOID user) {
    AuroraAooHandle *handle = (AuroraAooHandle *)user;
    (void)AooClient_send(handle->client, kAooInfinite);
    return 0;
}

static DWORD WINAPI aurora_aoo_receive_thread(LPVOID user) {
    AuroraAooHandle *handle = (AuroraAooHandle *)user;
    (void)AooClient_receive(handle->client, kAooInfinite);
    return 0;
}
#else
static void *aurora_aoo_send_thread(void *user) {
    AuroraAooHandle *handle = (AuroraAooHandle *)user;
    (void)AooClient_send(handle->client, kAooInfinite);
    return NULL;
}

static void *aurora_aoo_receive_thread(void *user) {
    AuroraAooHandle *handle = (AuroraAooHandle *)user;
    (void)AooClient_receive(handle->client, kAooInfinite);
    return NULL;
}
#endif

static void aurora_aoo_stop_network_threads(AuroraAooHandle *handle) {
    if (handle == NULL || !handle->network_threads_running || handle->client == NULL) {
        return;
    }

    (void)AooClient_stop(handle->client);
#ifdef _WIN32
    WaitForSingleObject(handle->send_thread, INFINITE);
    WaitForSingleObject(handle->receive_thread, INFINITE);
    CloseHandle(handle->send_thread);
    CloseHandle(handle->receive_thread);
    handle->send_thread = NULL;
    handle->receive_thread = NULL;
#else
    (void)pthread_join(handle->send_thread, NULL);
    (void)pthread_join(handle->receive_thread, NULL);
#endif
    handle->network_threads_running = 0;
}

static int32_t aurora_aoo_start_network_threads(AuroraAooHandle *handle) {
#ifdef _WIN32
    handle->send_thread = CreateThread(NULL, 0, aurora_aoo_send_thread, handle, 0, NULL);
    if (handle->send_thread == NULL) {
        return AURORA_AOO_ERR_THREAD;
    }
    handle->receive_thread = CreateThread(NULL, 0, aurora_aoo_receive_thread, handle, 0, NULL);
    if (handle->receive_thread == NULL) {
        (void)AooClient_stop(handle->client);
        WaitForSingleObject(handle->send_thread, INFINITE);
        CloseHandle(handle->send_thread);
        handle->send_thread = NULL;
        return AURORA_AOO_ERR_THREAD;
    }
#else
    if (pthread_create(&handle->send_thread, NULL, aurora_aoo_send_thread, handle) != 0) {
        return AURORA_AOO_ERR_THREAD;
    }
    if (pthread_create(&handle->receive_thread, NULL, aurora_aoo_receive_thread, handle) != 0) {
        (void)AooClient_stop(handle->client);
        (void)pthread_join(handle->send_thread, NULL);
        return AURORA_AOO_ERR_THREAD;
    }
#endif
    handle->network_threads_running = 1;
    return 0;
}

static void aurora_aoo_cleanup_runtime(AuroraAooHandle *handle) {
    if (handle == NULL) {
        return;
    }

    if (handle->started && handle->source != NULL) {
        (void)AooSource_stopStream(handle->source, 0);
        handle->started = 0;
    }

    aurora_aoo_stop_network_threads(handle);

    if (handle->client != NULL && handle->source != NULL && handle->source_registered) {
        (void)AooClient_removeSource(handle->client, handle->source);
        handle->source_registered = 0;
    }
    if (handle->client != NULL) {
        AooClient_free(handle->client);
        handle->client = NULL;
    }
    if (handle->source != NULL) {
        AooSource_free(handle->source);
        handle->source = NULL;
    }

    free(handle->planar_channels);
    free(handle->planar_storage);
    handle->planar_channels = NULL;
    handle->planar_storage = NULL;
    handle->prepared = 0;
    handle->sink_added = 0;
    handle->has_time_base = 0;
}

AURORA_SHIM_EXPORT uint32_t aurora_aoo_abi_version(void) {
    return AURORA_AOO_ABI_VERSION;
}

AURORA_SHIM_EXPORT void *aurora_aoo_create(void) {
    AooSettings settings = AOO_SETTINGS_INIT();
    if (aoo_initialize(&settings) != kAooOk) {
        return NULL;
    }

    AuroraAooHandle *handle = (AuroraAooHandle *)calloc(1, sizeof(AuroraAooHandle));
    if (handle == NULL) {
        aoo_terminate();
        return NULL;
    }
    return handle;
}

AURORA_SHIM_EXPORT int32_t aurora_aoo_prepare(
    void *opaque,
    const AuroraAooShimConfig *config
) {
    AuroraAooHandle *handle = (AuroraAooHandle *)opaque;
    if (handle == NULL || config == NULL) {
        return AURORA_AOO_ERR_ARGUMENT;
    }
    if (handle->prepared) {
        return AURORA_AOO_ERR_STATE;
    }
    if (config->channels <= 0 || config->sample_rate <= 0 ||
        config->block_frames <= 0 || config->source_id < 0 ||
        config->local_port < 0 || config->local_port > 65535) {
        return AURORA_AOO_ERR_ARGUMENT;
    }
    if (config->sink_host != NULL &&
        (config->sink_port <= 0 || config->sink_port > 65535 || config->sink_id < 0)) {
        return AURORA_AOO_ERR_ARGUMENT;
    }

    handle->channels = config->channels;
    handle->sample_rate = config->sample_rate;
    handle->block_frames = config->block_frames;

    handle->planar_channels = (AooSample **)calloc(
        (size_t)handle->channels,
        sizeof(AooSample *)
    );
    handle->planar_storage = (AooSample *)calloc(
        (size_t)handle->channels * (size_t)handle->block_frames,
        sizeof(AooSample)
    );
    if (handle->planar_channels == NULL || handle->planar_storage == NULL) {
        aurora_aoo_cleanup_runtime(handle);
        return AURORA_AOO_ERR_ALLOC;
    }
    for (int32_t channel = 0; channel < handle->channels; ++channel) {
        handle->planar_channels[channel] = handle->planar_storage +
            ((size_t)channel * (size_t)handle->block_frames);
    }

    handle->source = AooSource_new(config->source_id);
    handle->client = AooClient_new();
    if (handle->source == NULL || handle->client == NULL) {
        aurora_aoo_cleanup_runtime(handle);
        return AURORA_AOO_ERR_ALLOC;
    }

    AooClientSettings client_settings = AOO_CLIENT_SETTINGS_INIT();
    client_settings.portNumber = (AooUInt16)config->local_port;
    AooError error = AooClient_setup(handle->client, &client_settings);
    if (error != kAooOk) {
        aurora_aoo_cleanup_runtime(handle);
        return (int32_t)error;
    }

    error = AooSource_setup(
        handle->source,
        handle->channels,
        handle->sample_rate,
        handle->block_frames,
        0
    );
    if (error != kAooOk) {
        aurora_aoo_cleanup_runtime(handle);
        return (int32_t)error;
    }

    /* Aurora owns the rate controller for this adapter revision. */
    error = AooSource_setDynamicResampling(handle->source, kAooFalse);
    if (error != kAooOk) {
        aurora_aoo_cleanup_runtime(handle);
        return (int32_t)error;
    }

    AooFormatPcm format;
    AooFormatPcm_init(
        &format,
        handle->channels,
        handle->sample_rate,
        handle->block_frames,
        kAooPcmFloat32
    );
    error = AooSource_setFormat(handle->source, &format.header);
    if (error != kAooOk) {
        aurora_aoo_cleanup_runtime(handle);
        return (int32_t)error;
    }

    error = AooClient_addSource(handle->client, handle->source);
    if (error != kAooOk) {
        aurora_aoo_cleanup_runtime(handle);
        return (int32_t)error;
    }
    handle->source_registered = 1;

    if (config->sink_host != NULL) {
        AooAddrSize addrlen = (AooAddrSize)sizeof(handle->sink_address);
        error = aoo_resolveIpEndpoint(
            config->sink_host,
            (AooUInt16)config->sink_port,
            client_settings.socketType,
            &handle->sink_address,
            &addrlen
        );
        if (error != kAooOk) {
            aurora_aoo_cleanup_runtime(handle);
            return (int32_t)error;
        }
        handle->sink_endpoint.address = &handle->sink_address;
        handle->sink_endpoint.addrlen = addrlen;
        handle->sink_endpoint.id = config->sink_id;
        error = AooSource_addSink(handle->source, &handle->sink_endpoint, kAooTrue);
        if (error != kAooOk) {
            aurora_aoo_cleanup_runtime(handle);
            return (int32_t)error;
        }
        handle->sink_added = 1;
    }

    int32_t thread_error = aurora_aoo_start_network_threads(handle);
    if (thread_error != 0) {
        aurora_aoo_cleanup_runtime(handle);
        return thread_error;
    }

    handle->prepared = 1;
    return 0;
}

AURORA_SHIM_EXPORT int32_t aurora_aoo_start(void *opaque) {
    AuroraAooHandle *handle = (AuroraAooHandle *)opaque;
    if (handle == NULL || !handle->prepared || handle->source == NULL) {
        return AURORA_AOO_ERR_STATE;
    }
    if (handle->started) {
        return AURORA_AOO_ERR_STATE;
    }

    AooError error = AooSource_startStream(handle->source, 0, NULL);
    if (error != kAooOk) {
        return (int32_t)error;
    }
    handle->started = 1;
    handle->has_time_base = 0;
    return 0;
}

AURORA_SHIM_EXPORT int32_t aurora_aoo_submit(
    void *opaque,
    const float *interleaved,
    uint64_t media_frame_index
) {
    AuroraAooHandle *handle = (AuroraAooHandle *)opaque;
    if (handle == NULL || !handle->started || interleaved == NULL) {
        return AURORA_AOO_ERR_STATE;
    }

    for (int32_t frame = 0; frame < handle->block_frames; ++frame) {
        for (int32_t channel = 0; channel < handle->channels; ++channel) {
            size_t interleaved_index =
                (size_t)frame * (size_t)handle->channels + (size_t)channel;
            handle->planar_channels[channel][frame] = (AooSample)interleaved[interleaved_index];
        }
    }

    if (!handle->has_time_base) {
        handle->base_media_frame = media_frame_index;
        handle->base_ntp = aoo_getCurrentNtpTime();
        handle->has_time_base = 1;
    }
    if (media_frame_index < handle->base_media_frame) {
        return AURORA_AOO_ERR_TIMELINE;
    }

    const uint64_t delta_frames = media_frame_index - handle->base_media_frame;
    const AooSeconds delta_seconds =
        (AooSeconds)delta_frames / (AooSeconds)handle->sample_rate;
    const AooSeconds base_seconds = aoo_ntpTimeToSeconds(handle->base_ntp);
    const AooNtpTime timestamp = aoo_ntpTimeFromSeconds(base_seconds + delta_seconds);

    AooError error = AooSource_process(
        handle->source,
        handle->planar_channels,
        handle->block_frames,
        timestamp
    );
    if (error != kAooOk) {
        return (int32_t)error;
    }
    error = AooClient_notify(handle->client);
    if (error != kAooOk && error != kAooErrorIdle) {
        return (int32_t)error;
    }
    return 0;
}

AURORA_SHIM_EXPORT int32_t aurora_aoo_stop(void *opaque) {
    AuroraAooHandle *handle = (AuroraAooHandle *)opaque;
    if (handle == NULL || !handle->prepared || handle->source == NULL) {
        return AURORA_AOO_ERR_STATE;
    }
    if (!handle->started) {
        return 0;
    }

    AooError error = AooSource_stopStream(handle->source, 0);
    if (error != kAooOk) {
        return (int32_t)error;
    }
    handle->started = 0;
    handle->has_time_base = 0;
    return 0;
}

AURORA_SHIM_EXPORT int32_t aurora_aoo_reset(void *opaque) {
    AuroraAooHandle *handle = (AuroraAooHandle *)opaque;
    if (handle == NULL || !handle->prepared || handle->source == NULL) {
        return AURORA_AOO_ERR_STATE;
    }
    AooError error = AooSource_reset(handle->source);
    if (error != kAooOk) {
        return (int32_t)error;
    }
    handle->has_time_base = 0;
    return 0;
}

AURORA_SHIM_EXPORT void aurora_aoo_destroy(void *opaque) {
    AuroraAooHandle *handle = (AuroraAooHandle *)opaque;
    if (handle == NULL) {
        return;
    }
    aurora_aoo_cleanup_runtime(handle);
    free(handle);
    aoo_terminate();
}
