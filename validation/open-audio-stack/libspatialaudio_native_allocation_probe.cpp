#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <new>

namespace {
thread_local bool g_audit_active = false;
thread_local std::size_t g_allocations = 0;

constexpr std::uint32_t kSampleRate = 48000;
constexpr std::uint32_t kBlockFrames = 256;
constexpr std::uint32_t kOutputs = 12;
constexpr std::uint32_t kObjects = 1;
constexpr std::size_t kAuditIterations = 512;
}  // namespace

void* operator new(std::size_t size) {
    if (g_audit_active) {
        ++g_allocations;
    }
    if (void* pointer = std::malloc(size)) {
        return pointer;
    }
    throw std::bad_alloc();
}

void* operator new[](std::size_t size) {
    if (g_audit_active) {
        ++g_allocations;
    }
    if (void* pointer = std::malloc(size)) {
        return pointer;
    }
    throw std::bad_alloc();
}

void operator delete(void* pointer) noexcept {
    std::free(pointer);
}

void operator delete[](void* pointer) noexcept {
    std::free(pointer);
}

void operator delete(void* pointer, std::size_t) noexcept {
    std::free(pointer);
}

void operator delete[](void* pointer, std::size_t) noexcept {
    std::free(pointer);
}

extern "C" {
std::uint32_t aurora_spaudio_abi_version();
void* aurora_spaudio_create();
int aurora_spaudio_configure(void*, std::uint32_t, std::uint32_t, std::uint32_t);
int aurora_spaudio_render(
    void*,
    std::uint32_t,
    const float*,
    const float*,
    const float* const*,
    float**);
int aurora_spaudio_reset(void*);
std::uint32_t aurora_spaudio_output_channels(void*);
std::uint32_t aurora_spaudio_latency_frames();
void aurora_spaudio_destroy(void*);
}

int main() {
    if (aurora_spaudio_abi_version() != 1) {
        std::cerr << "unexpected shim ABI\n";
        return EXIT_FAILURE;
    }
    void* handle = aurora_spaudio_create();
    if (handle == nullptr) {
        std::cerr << "shim create failed\n";
        return EXIT_FAILURE;
    }
    if (aurora_spaudio_configure(handle, kSampleRate, kBlockFrames, kObjects) != 0 ||
        aurora_spaudio_output_channels(handle) != kOutputs ||
        aurora_spaudio_latency_frames() != 255) {
        std::cerr << "shim configure contract failed\n";
        aurora_spaudio_destroy(handle);
        return EXIT_FAILURE;
    }

    std::array<float, kBlockFrames> input{};
    for (std::size_t frame = 0; frame < input.size(); ++frame) {
        input[frame] = static_cast<float>((frame % 31) + 1) * 0.001f;
    }
    const float* inputs[kObjects] = {input.data()};
    const float positions[kObjects * 3] = {0.0f, 1.0f, 0.0f};
    const float gains[kObjects] = {1.0f};
    std::array<std::array<float, kBlockFrames>, kOutputs> output{};
    std::array<float*, kOutputs> output_ptrs{};
    for (std::size_t channel = 0; channel < output.size(); ++channel) {
        output_ptrs[channel] = output[channel].data();
    }

    // First call calculates object gains; second reaches unchanged-metadata
    // steady state after the 255-sample direct compensation path is primed.
    for (int warmup = 0; warmup < 2; ++warmup) {
        for (auto& channel : output) {
            channel.fill(0.0f);
        }
        if (aurora_spaudio_render(
                handle,
                kObjects,
                positions,
                gains,
                inputs,
                output_ptrs.data()) != 0) {
            std::cerr << "warmup render failed\n";
            aurora_spaudio_destroy(handle);
            return EXIT_FAILURE;
        }
    }

    g_allocations = 0;
    g_audit_active = true;
    for (std::size_t iteration = 0; iteration < kAuditIterations; ++iteration) {
        for (auto& channel : output) {
            channel.fill(0.0f);
        }
        if (aurora_spaudio_render(
                handle,
                kObjects,
                positions,
                gains,
                inputs,
                output_ptrs.data()) != 0) {
            g_audit_active = false;
            std::cerr << "audited render failed\n";
            aurora_spaudio_destroy(handle);
            return EXIT_FAILURE;
        }
    }
    g_audit_active = false;

    const std::size_t allocations = g_allocations;
    aurora_spaudio_destroy(handle);
    if (allocations != 0) {
        std::cerr << "steady-state native render allocated " << allocations << " times\n";
        return EXIT_FAILURE;
    }

    std::cout << "aurora-libspatialaudio-native-allocation: PASS iterations="
              << kAuditIterations << " allocations=0\n";
    return EXIT_SUCCESS;
}
