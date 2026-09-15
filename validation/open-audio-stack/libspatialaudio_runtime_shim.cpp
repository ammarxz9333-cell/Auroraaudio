#include <spatialaudio/Renderer.h>

#include <cstdint>
#include <memory>
#include <new>
#include <vector>

namespace {

constexpr std::uint32_t kAbiVersion = 1;
constexpr std::uint32_t kOutputChannels = 12;
constexpr std::uint32_t kLatencyFrames = 255;
constexpr std::uint32_t kSampleRate = 48000;
constexpr std::uint32_t kBlockFrames = 256;

struct AuroraSpaudioHandle {
    std::unique_ptr<spaudio::Renderer> renderer;
    std::vector<spaudio::ObjectMetadata> metadata;
    std::uint32_t max_objects = 0;
    bool configured = false;
};

}  // namespace

extern "C" {

std::uint32_t aurora_spaudio_abi_version() {
    return kAbiVersion;
}

void* aurora_spaudio_create() {
    auto* handle = new (std::nothrow) AuroraSpaudioHandle();
    return handle;
}

int aurora_spaudio_configure(
    void* opaque,
    std::uint32_t sample_rate,
    std::uint32_t block_frames,
    std::uint32_t max_objects) {
    if (opaque == nullptr || sample_rate != kSampleRate ||
        block_frames != kBlockFrames || max_objects == 0) {
        return -1;
    }
    auto* handle = static_cast<AuroraSpaudioHandle*>(opaque);
    try {
        spaudio::StreamInformation stream_info;
        stream_info.nChannels = max_objects;
        stream_info.typeDefinition.assign(
            max_objects, spaudio::TypeDefinition::Objects);

        auto renderer = std::make_unique<spaudio::Renderer>();
        if (!renderer->Configure(
                spaudio::OutputLayout::SevenPointOnePointFour,
                1,
                sample_rate,
                block_frames,
                stream_info)) {
            return -2;
        }
        if (renderer->GetSpeakerCount() != kOutputChannels) {
            return -3;
        }

        std::vector<spaudio::ObjectMetadata> metadata(max_objects);
        for (std::uint32_t index = 0; index < max_objects; ++index) {
            auto& item = metadata[index];
            item.trackInd = index;
            item.blockLength = block_frames;
            item.cartesian = true;
            item.gain = 1.0;
            item.diffuse = 0.0;
            item.position = spaudio::CartesianPosition<double>{0.0, 1.0, 0.0};
        }

        handle->renderer = std::move(renderer);
        handle->metadata = std::move(metadata);
        handle->max_objects = max_objects;
        handle->configured = true;
        return 0;
    } catch (...) {
        return -4;
    }
}

int aurora_spaudio_render(
    void* opaque,
    std::uint32_t object_count,
    const float* positions_xyz,
    const float* gains,
    const float* const* inputs,
    float** outputs) {
    if (opaque == nullptr || outputs == nullptr) {
        return -1;
    }
    auto* handle = static_cast<AuroraSpaudioHandle*>(opaque);
    if (!handle->configured || !handle->renderer ||
        object_count > handle->max_objects) {
        return -2;
    }
    if (object_count > 0 &&
        (positions_xyz == nullptr || gains == nullptr || inputs == nullptr)) {
        return -3;
    }

    try {
        for (std::uint32_t index = 0; index < object_count; ++index) {
            if (inputs[index] == nullptr) {
                return -4;
            }
            auto& item = handle->metadata[index];
            auto& position = item.position.cartesianPosition();
            const std::uint32_t base = index * 3;
            position.x = positions_xyz[base];
            position.y = positions_xyz[base + 1];
            position.z = positions_xyz[base + 2];
            item.gain = gains[index];
            item.blockLength = kBlockFrames;
            handle->renderer->AddObject(
                const_cast<float*>(inputs[index]),
                kBlockFrames,
                item);
        }
        handle->renderer->GetRenderedAudio(outputs, kBlockFrames);
        return 0;
    } catch (...) {
        return -5;
    }
}

int aurora_spaudio_reset(void* opaque) {
    if (opaque == nullptr) {
        return -1;
    }
    auto* handle = static_cast<AuroraSpaudioHandle*>(opaque);
    if (!handle->configured || !handle->renderer) {
        return -2;
    }
    try {
        handle->renderer->Reset();
        return 0;
    } catch (...) {
        return -3;
    }
}

std::uint32_t aurora_spaudio_output_channels(void* opaque) {
    if (opaque == nullptr) {
        return 0;
    }
    auto* handle = static_cast<AuroraSpaudioHandle*>(opaque);
    if (!handle->configured || !handle->renderer) {
        return 0;
    }
    return handle->renderer->GetSpeakerCount();
}

std::uint32_t aurora_spaudio_latency_frames() {
    return kLatencyFrames;
}

void aurora_spaudio_destroy(void* opaque) {
    delete static_cast<AuroraSpaudioHandle*>(opaque);
}

}  // extern "C"
