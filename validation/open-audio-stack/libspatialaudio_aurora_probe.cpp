#include <spatialaudio/Renderer.h>

#include <cmath>
#include <cstdlib>
#include <iostream>
#include <vector>

using namespace spaudio;

int main() {
    constexpr unsigned int sample_rate = 48000;
    constexpr unsigned int block_frames = 256;
    constexpr unsigned int expected_outputs = 12;
    constexpr unsigned int rendered_blocks = 2;

    std::vector<float> input(block_frames);
    for (unsigned int i = 0; i < block_frames; ++i) {
        input[i] = 0.25f * std::sin(2.0 * M_PI * 440.0 * static_cast<double>(i) /
                                  static_cast<double>(sample_rate));
    }

    StreamInformation stream_info;
    stream_info.nChannels = 1;
    stream_info.typeDefinition.push_back(TypeDefinition::Objects);

    Renderer renderer;
    if (!renderer.Configure(
            OutputLayout::SevenPointOnePointFour,
            1,
            sample_rate,
            block_frames,
            stream_info)) {
        std::cerr << "libspatialaudio Configure failed for 7.1.4\n";
        return EXIT_FAILURE;
    }

    if (renderer.GetSpeakerCount() != expected_outputs) {
        std::cerr << "unexpected 7.1.4 speaker count: " << renderer.GetSpeakerCount() << "\n";
        return EXIT_FAILURE;
    }

    ObjectMetadata metadata;
    metadata.trackInd = 0;
    metadata.blockLength = block_frames;
    metadata.cartesian = false;
    metadata.position.polarPosition().azimuth = 0.0;
    metadata.position.polarPosition().elevation = 0.0;
    metadata.position.polarPosition().distance = 1.0;
    metadata.gain = 1.0;

    std::vector<std::vector<float>> output(expected_outputs,
                                           std::vector<float>(block_frames, 0.0f));
    std::vector<float*> output_ptrs;
    output_ptrs.reserve(expected_outputs);
    for (auto& channel : output) {
        output_ptrs.push_back(channel.data());
    }

    double total_energy = 0.0;
    unsigned int active_channels = 0;
    for (unsigned int block = 0; block < rendered_blocks; ++block) {
        for (auto& channel : output) {
            std::fill(channel.begin(), channel.end(), 0.0f);
        }

        renderer.AddObject(input.data(), block_frames, metadata);
        renderer.GetRenderedAudio(output_ptrs.data(), block_frames);

        for (const auto& channel : output) {
            double channel_energy = 0.0;
            for (float sample : channel) {
                if (!std::isfinite(sample)) {
                    std::cerr << "non-finite libspatialaudio output\n";
                    return EXIT_FAILURE;
                }
                channel_energy += static_cast<double>(sample) * static_cast<double>(sample);
            }
            if (channel_energy > 1.0e-12) {
                ++active_channels;
            }
            total_energy += channel_energy;
        }
    }

    // The upstream object renderer intentionally delays the direct signal by
    // (512 - 1) / 2 = 255 samples to align it with the diffuse decorrelator.
    // Two 256-frame calls therefore exercise steady-state output without
    // weakening the finite/non-silent contract around that documented latency.
    if (!(total_energy > 1.0e-10) || active_channels == 0) {
        std::cerr << "libspatialaudio produced no 7.1.4 output after compensation latency\n";
        return EXIT_FAILURE;
    }

    std::cout << "aurora-libspatialaudio-contract: PASS outputs=" << expected_outputs
              << " rate=" << sample_rate << " block=" << block_frames
              << " blocks=" << rendered_blocks << " active=" << active_channels
              << " energy=" << total_energy << "\n";
    return EXIT_SUCCESS;
}
