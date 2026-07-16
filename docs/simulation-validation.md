# Simulation Validation

`simulate-duplex` writes callback, frame, fill, ratio, ppm, saturation,
underflow, overflow, state, execution-time, acceleration, and deterministic
checksum fields. Long runs validate bounded scheduling and controller behavior
without waiting for wall-clock time. A separate sample-path checksum proves that
the run also exercised Aurora's actual SPSC, adaptive resampler, renderer, and
DSP path without CPAL.

`simulate-latency` creates a deterministic bipolar sequence and routes it over
a virtual cable with exact delay, bounded jitter, gain, polarity, noise, and an
optional one-pole low-pass. The result compares estimator output with simulator
truth and uses `simulated_virtual_loopback_truth`, never `measured`.

`simulate-output-validation` sends unique signed impulses through canonical
role indices. It checks unique routing, inactive silence, gain, polarity, and
5.1.2 metadata limitations. WAVE_FORMAT_EXTENSIBLE can identify top-front
positions but cannot describe an up-firing transducer's acoustic intent.

Simulation cannot validate host-driver scheduling, DAC/ADC latency, analog
noise, USB behavior, endpoint GUID stability, or physical loopback wiring.
