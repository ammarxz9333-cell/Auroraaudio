# Simulation Backend

`aurora-realtime-audio-sim` implements Aurora's input/output backend traits
without CPAL. Its public surface contains only Aurora-owned device, callback,
fault, scheduler, cable, configuration, and report types.

Built-in profiles cover stereo consumer, USB 5.1, USB 7.1, 12-channel
development, and intentionally broken drivers. Endpoints advertise stable IDs,
direction, rates, channel counts, formats, latency, callback policy, clock ppm,
and jitter. The broken profile also exposes duplicate output names so selector
collision handling is exercised.

The accelerated long-duration runner models every callback event and applies
the production PI drift-controller contract to ring fill. Sample-path ASRC and
block chunking remain covered by realtime-engine callback tests. Every run also
executes a bounded 128-callback probe through the real contiguous SPSC ring,
Rubato ASRC, and `RealTimeEngine`, recording a finite-sample checksum. Reports
label their source `simulated_virtual_audio_hardware`; they are not hardware
results.

No simulator dependency or type appears in `aurora-core`, the renderer API, or
the CPAL adapter.
