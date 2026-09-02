# Aurora R1 Direct ALSA Output

R1 is the lean appliance-oriented output path layered on top of the i.MX93 R0 architecture. It deliberately bypasses PipeWire/WirePlumber for the rendered 7.1.4 output path, while R0 remains intact as the fallback until physical i.MX93/TDM/DAC validation is complete.

## Selected R1 chain

```text
TV eARC / SiI9437 source clock
  -> SAI1 capture / IEC61937
  -> Aurora E-AC-3 extractor
  -> Harletty JOC bridge
  -> Omniphony 7.1.4
  -> raw-f32 stdout, 12ch, 48 kHz
  -> aurora-alsa-out
       - fixed-capacity preallocated PCM ring
       - smoothed PI clock controller
       - anti-windup + correction slew limit
       - 32-tap / 2048-phase bandlimited fractional resampler
       - canonical 12 -> 16 slot pack
       - -3 dB default software headroom
       - f32 -> S32_LE saturation
       - exact ALSA hw/sw parameter ownership
       - xrun/suspend recovery
       - measured queue + resampler + ALSA/DMA latency
       - A/V delay publication on a non-audio telemetry thread
  -> hw:AuroraTDM16,0
  -> SAI3 TDM512
  -> dual AK4458
```

## Why `aurora-alsa-out` still exists

The pinned Omniphony Linux realtime backend is PipeWire. Its `file` backend can stream raw interleaved float PCM to stdout/FIFO, but it intentionally has no DAC device clock or adaptive rate matching. A direct `orender | aplay` chain would therefore lose the function that protects a long-running appliance from source-vs-DAC oscillator drift.

No modifiable Omniphony fork is currently part of this repository, so R1 keeps a small dedicated bridge instead of carrying a private renderer fork. The bridge owns only the functions required at the hardware boundary:

1. direct ALSA hardware ownership;
2. DAC-clock pacing;
3. source/DAC rate matching;
4. canonical channel-to-TDM-slot packing;
5. explicit sample conversion and clipping rules;
6. deterministic ALSA buffering/recovery;
7. output-chain latency measurement and publication.

If a maintained Omniphony native-ALSA backend becomes available later, these same contracts can move into the renderer and the process boundary can be removed without changing the TDM/channel design.

## Realtime / allocation model

After startup, the steady-state output path does not allocate a new PCM block for every renderer chunk.

- stdin parsing reuses one raw-byte block and one 256-frame decoded block;
- the source/DAC boundary is a preallocated fixed-capacity `Frame12` ring;
- the resampler owns a fixed 32-frame history window and a precomputed phase table;
- the 16-channel S32 output period buffer is allocated once and overwritten in place;
- the A/V delay file is written by a separate telemetry thread, not the audio thread.

The audio thread therefore stays focused on queue consumption, clock correction, fractional interpolation, channel packing and blocking ALSA writes.

## Clock model

The live chain is source-clock paced at capture. `arecord` receives the eARC-derived SAI1 stream, so E-AC-3 access units arrive at the decoder at the source's long-term rate. Omniphony can compute individual blocks faster than realtime, but sustained production is constrained by that capture clock.

The AK4458 side has its own 48 kHz playback clock. Queue fill is the control error:

```text
queue above target -> positive ppm -> consume source slightly faster
queue below target -> negative ppm -> consume source slightly slower
```

The R1 controller adds four protections around the basic PI loop:

- low-pass filtering of block-level queue jitter;
- anti-windup while the output correction is saturated;
- a default hard correction clamp of ±300 ppm;
- a correction slew limit so queue discontinuities do not become abrupt ratio steps.

After an ALSA xrun/suspend recovery, accumulated PI history is reset because the timing discontinuity invalidates the previous integral state.

The ±300 ppm limit is intentionally conservative. A system that needs materially more continuous correction should be treated as a clock-tree/configuration fault, not hidden by a more aggressive resampler.

## Bandlimited fractional resampler

The original R1 prototype used four-point cubic interpolation. That implementation was removed before hardware adoption because a continuously moving fractional phase can produce excessive upper-band interpolation error even when the average ratio correction is only a few hundred ppm.

Current R1 uses a 32-tap, 2048-phase Lanczos-windowed sinc table:

```text
sample rate:       48 kHz
channels:          12
filter length:     32 taps
fractional phases: 2048
normal correction: <= ±300 ppm
internal safety:   source step limited to 0.999..1.001
lookahead:         16 frames
```

The kernels are precomputed once at startup. The steady-state interpolator is allocation-free.

CI quality gates currently exercise constant-signal preservation plus 18 kHz and 20 kHz sine interpolation at both -300 ppm and +300 ppm. These software tests passed. They are not a substitute for analog THD+N/FFT measurements through the real i.MX93 -> SAI3 -> AK4458 path.

## ALSA ownership

All direct `libasound` unsafe FFI is isolated in `alsa_pcm.rs`. The runtime requests the production contract explicitly:

```text
48,000 Hz
16 channels
S32_LE
RW_INTERLEAVED
```

R1 does not use `default`, `plughw` or a plugin layer that may silently remap channels or convert formats.

Hardware and software PCM parameters are explicit. The applied period and buffer sizes are read back from ALSA, and invalid geometry is rejected. Software thresholds set:

- `avail_min = one period`;
- `start_threshold = buffer - one period`;
- `stop_threshold = buffer`.

Partial writes are completed. Recoverable xrun/suspend conditions go through `snd_pcm_recover`; unrecoverable ALSA errors are fatal.

## Channel contract

Input from Omniphony:

```text
0 FL
1 FR
2 C
3 LFE
4 BL
5 BR
6 SL
7 SR
8 TFL
9 TFR
10 TRL
11 TRR
```

Output to ALSA/TDM:

```text
0..11  same channels, unchanged order
12     AUX0 = digital zero
13     AUX1 = digital zero
14     AUX2 = digital zero
15     AUX3 = digital zero
```

No semantic channel remapping is delegated to ALSA or PipeWire in R1.

## Sample conversion

Omniphony produces interleaved little-endian `f32` at 48 kHz / 12 channels. R1 applies the configured linear gain, then converts explicitly to signed 32-bit PCM.

Default bring-up headroom is -3 dBFS. Non-finite float samples become digital zero. Samples outside full scale saturate at `i32::MIN/MAX`; they never wrap.

## A/V latency ownership

R1 cannot rely on Omniphony's realtime-output latency snapshot because the renderer is using its raw file/stdout backend. `aurora-alsa-out` therefore owns output latency reporting.

The measured estimate is:

```text
software PCM queue
+ 16-frame resampler lookahead
+ snd_pcm_delay() ALSA/DMA queued frames
```

The latest total is sampled at a low rate and published using the existing `/tmp/omniphony_delay` sign convention. Filesystem I/O occurs on a dedicated telemetry thread; the audio thread only updates an atomic value. A stale delay file is removed at reporter startup and normal shutdown.

Target hardware must still verify that `snd_pcm_delay()` on the selected i.MX93 BSP accounts for the relevant DMA/device queue accurately enough for long-form A/V sync.

## CI evidence

The hardened R1 software path has passed the dedicated CI matrix with:

- Linux `cargo fmt`;
- Linux build and direct `libasound` link;
- strict Clippy with warnings denied;
- R1 unit tests, including the upper-band ±300 ppm resampler gates;
- `--help` binary smoke;
- actual runtime open/write against ALSA `null` as 48 kHz / 16ch / S32_LE;
- finite-source fail-closed behavior;
- shell syntax for live/deploy scripts;
- systemd service contract proving no PipeWire/WirePlumber dependency;
- Rust 1.78 MSRV check and tests;
- Windows compile proving Linux-only FFI is correctly cfg-isolated.

This proves the software contracts exercised by CI. It does **not** prove the i.MX93 SAI3 driver, actual TDM electrical stream, DAC slot order or analog audio performance.

## Fail-closed rules

- No fallback to the default ALSA device.
- No `plughw` implicit conversion in the production path.
- Raw stream misalignment, source timeout or source EOF is fatal.
- Unrecoverable ALSA errors are fatal.
- Four reserve TDM slots are always digital zero.
- PI history is reset after a playback recovery discontinuity.
- Amplifier unmute remains outside the process until the hardware fail-safe mute gate is physically validated.

## Remaining physical gates

R1 remains Draft until the actual i.MX93 carrier proves all of the following:

1. `hw:AuroraTDM16,0` opens at exact 48 kHz / 16ch / S32_LE on the target BSP.
2. SAI3 sustains TDM512 playback with the real DMA configuration.
3. Logical channels 0..11 reach the intended two AK4458 DAC channels and slots 12..15 stay digital zero.
4. A long-running source with measured clock offset keeps the software queue bounded without audible modulation or discontinuity.
5. Target-BSP xrun/suspend recovery behaves as expected.
6. Digital FFT and analog THD+N/swept-sine measurements pass at 0 and ±300 ppm correction.
7. A/V sync remains bounded over at least a two-hour movie/soak run.
8. CPU load on the i.MX93 leaves adequate realtime margin alongside Harletty/JOC + Omniphony rendering.
9. G7 amplifier mute/gain safety is physically validated before normal speaker unmute.

## R0 fallback

R0/PipeWire remains preserved and usable. R1 should become the preferred appliance path only after the physical gates above are green. No CI result is treated as proof of the unbuilt carrier, SAI3/TDM electrical path or AK4458 analog output.
