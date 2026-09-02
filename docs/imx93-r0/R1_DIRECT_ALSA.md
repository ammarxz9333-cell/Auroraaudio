# Aurora R1 Direct ALSA Output

R1 is an experimental lean output path layered on top of the i.MX93 R0 architecture. It does **not** replace the R0/PipeWire path until the direct-ALSA implementation passes CI and target-hardware validation.

## Selected R1 chain

```text
TV eARC / SiI9437 source clock
  -> SAI1 capture / IEC61937
  -> Aurora E-AC-3 extractor
  -> Harletty JOC bridge
  -> Omniphony 7.1.4
  -> raw-f32 stdout, 12ch, 48 kHz
  -> aurora-alsa-out
       - bounded source queue
       - PI queue controller
       - tiny adaptive fractional resampling
       - canonical 12 -> 16 slot pack
       - -3 dB default software headroom
       - f32 -> S32_LE saturation
       - ALSA xrun/suspend recovery
  -> hw:AuroraTDM16,0
  -> SAI3 TDM512
  -> dual AK4458
```

## Why the extra bridge exists

The pinned Omniphony Linux realtime backend is PipeWire. Its alternative `file` backend can stream raw interleaved float PCM to stdout/FIFO, but it intentionally has no device clock or adaptive resampling. Directly piping that output to `aplay` would therefore remove the rate-matching function that protects a long-running appliance from oscillator drift.

`aurora-alsa-out` restores only the functions Aurora needs while avoiding a desktop/session audio graph:

1. ALSA hardware device ownership;
2. DAC-clock pacing;
3. small source/DAC clock correction;
4. explicit 12-channel to 16-slot mapping;
5. explicit sample conversion and clipping behavior;
6. xrun recovery and telemetry.

## Clock model

The live chain is not an offline file render. `arecord` captures the eARC-derived SAI1 stream using the source clock, so E-AC-3 access units arrive at the decoder at the source's long-term rate. Omniphony may render individual blocks faster than realtime, but its average live production rate is constrained by that clocked input.

The AK4458 side has an independent 48 kHz clock. A bounded PCM queue sits between the two domains. Queue fill is the error signal for a PI controller:

```text
queue above target -> positive ppm -> consume source slightly faster
queue below target -> negative ppm -> consume source slightly slower
```

The correction is clamped to ±300 ppm by default. Values outside normal oscillator tolerance are treated as configuration/fault territory, not as a way to hide a broken clock tree.

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

## Sample format

Omniphony stream:

```text
48,000 Hz
12 channels
interleaved little-endian f32
```

ALSA hardware stream:

```text
48,000 Hz
16 channels
S32_LE
RW_INTERLEAVED
```

The default R1 gain is -3 dBFS for bring-up headroom. Non-finite float samples are converted to digital zero. Values outside full scale saturate explicitly rather than wrapping.

## Resampler scope

R1 uses a streaming four-point cubic interpolator only for tiny clock corrections. It is hard-limited internally to a source step of 0.999..1.001 and is **not** a general-purpose sample-rate converter. The PI controller default clamp is narrower: ±300 ppm.

Before R1 can supersede PipeWire, target measurements must include:

- swept-sine / FFT comparison at 0 ppm and ±300 ppm;
- THD+N impact at high frequencies;
- long-run queue stability with deliberately offset clocks;
- no audible discontinuity when correction crosses 0 ppm;
- xrun recovery behavior;
- A/V sync drift over at least two hours.

If the cubic interpolator fails the audio-quality gate, replace only the fractional resampler with a high-quality variable-ratio polyphase/sinc implementation. The ALSA/channel/watchdog architecture remains valid.

## Fail-closed rules

- No fallback to the default ALSA device.
- No `plughw` implicit format/channel conversion in the selected production path.
- Raw stream misalignment, source timeout or source EOF is fatal.
- ALSA write errors are recovered only through `snd_pcm_recover`; unrecoverable errors are fatal.
- Four reserve TDM slots are always digital zero.
- Amplifier unmute remains outside the process until the hardware fail-safe mute gate is physically validated.

## R0 fallback

R0 remains the current safer prototype because PipeWire already provides a mature device-clocked realtime backend and adaptive rate control. R1 becomes the preferred appliance path only after its CI and physical validation gates are green.
