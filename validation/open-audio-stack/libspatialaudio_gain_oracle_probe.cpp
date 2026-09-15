#include <spatialaudio/LoudspeakerLayouts.h>
#include <spatialaudio/RendererMetadata.h>
#include <spatialaudio/adm/GainCalculator.h>

#include <array>
#include <cmath>
#include <cstdlib>
#include <iostream>
#include <vector>

namespace {

constexpr std::size_t kChannels = 12;

bool check_nominal_direction(
    spaudio::adm::ObjectGainCalculator& calculator,
    double azimuth,
    double elevation,
    std::size_t expected_channel) {
    spaudio::ObjectMetadata metadata;
    metadata.cartesian = false;
    metadata.position = spaudio::PolarPosition<double>{azimuth, elevation, 1.0};
    metadata.gain = 1.0;
    metadata.diffuse = 0.0;
    metadata.blockLength = 256;

    std::vector<double> direct(kChannels, 0.0);
    std::vector<double> diffuse(kChannels, 0.0);
    calculator.CalculateGains(metadata, direct, diffuse);

    if (direct[expected_channel] < 0.999999) {
        std::cerr << "expected channel " << expected_channel
                  << " not unity for az=" << azimuth
                  << " el=" << elevation
                  << " gain=" << direct[expected_channel] << "\n";
        return false;
    }
    for (std::size_t channel = 0; channel < direct.size(); ++channel) {
        if (channel != expected_channel && std::abs(direct[channel]) > 1.0e-9) {
            std::cerr << "unexpected gain channel=" << channel
                      << " value=" << direct[channel]
                      << " for expected=" << expected_channel << "\n";
            return false;
        }
        if (std::abs(diffuse[channel]) > 1.0e-12) {
            std::cerr << "unexpected diffuse gain channel=" << channel
                      << " value=" << diffuse[channel] << "\n";
            return false;
        }
    }
    return true;
}

}  // namespace

int main() {
    const auto layout = spaudio::Layout::getMatchingLayout("4+7+0");
    if (layout.getNumChannels() != kChannels) {
        std::cerr << "unexpected 4+7+0 channel count\n";
        return EXIT_FAILURE;
    }

    spaudio::adm::ObjectGainCalculator calculator(layout);
    const std::array<std::array<double, 3>, 3> cases{{
        {{30.0, 0.0, 0.0}},
        {{-30.0, 0.0, 1.0}},
        {{0.0, 0.0, 2.0}},
    }};
    for (const auto& item : cases) {
        if (!check_nominal_direction(
                calculator,
                item[0],
                item[1],
                static_cast<std::size_t>(item[2]))) {
            return EXIT_FAILURE;
        }
    }

    std::cout << "aurora-libspatialaudio-gain-oracle: PASS nominal=FL,FR,FC\n";
    return EXIT_SUCCESS;
}
