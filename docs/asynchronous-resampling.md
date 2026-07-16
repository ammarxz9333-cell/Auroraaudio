# Asynchronous Resampling

Aurora owns the `AsynchronousResampler` API. It configures nominal input/output
rates, one coherent channel count, and a fixed maximum output block; accepts
smooth absolute ratio updates; reports required input frames and latency; and
processes caller-owned interleaved buffers without allocation.

The first implementation adapts Rubato 0.16.2 `SincFixedOut<f32>`. Third-party
types remain private. The selected quality profile is documented in code:

- 64-tap windowed sinc
- Blackman-Harris 2 window
- 0.95 relative cutoff
- 128 fractional sinc positions
- linear interpolation between positions
- maximum relative ratio 1.0005, matching the initial +/-500 ppm controller

Rubato produces one fixed 256-frame block in the normal 48 kHz configuration.
Aurora caches that block in preallocated interleaved storage so variable CPAL
callback sizes and tails do not resize buffers. Ratio updates use Rubato's ramped
mode. The measured algorithmic delay is reported separately from ring buffering,
renderer latency, and any unknown device latency.

Identity, 44.1-to-48 kHz, multichannel coherence, finite output, deterministic
output, silence, and zero steady-state allocation are tested. This is the normal
duplex path; crossfade/sample slip remains test/fallback reference behavior.

