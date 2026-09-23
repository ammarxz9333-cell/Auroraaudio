# Raspberry Pi 5 eARC runtime profile

This profile turns the existing Aurora software stack into one deployable home-theater path on a Raspberry Pi 5 without changing Aurora core APIs.

## Selected runtime

```text
TV internal apps / HDMI sources
        |
      eARC
        |
Lindy 38368 / SiI9437
        |
  I2S tap, Pi 5 slave
  BCLK GPIO18, WS GPIO19
  SD0 GPIO20, SD1 GPIO22, SD2 GPIO24, SD3 GPIO26
        |
ALSA card "eARC"
S32_LE, 2-channel IEC61937 carrier @ 192 kHz for DD+
        |
aurora_alsa_iec61937_stream.py
S32 slots -> canonical 16-bit IEC61937
        |
      stdout
        |
Omniphony 0.6.0 orender stdin
  -> IEC61937 SpdifParser
  -> Harletty bridge 0.8.0
  -> E-AC-3 / JOC decode + object metadata
  -> Omniphony spatial render
        |
Aurora custom 11.1.4 = 16 output channels
  -> LR4 bass management: sub <80 Hz, floor >80 Hz, heights >100 Hz
  -> -3 dB baseline headroom + auto-gain ceiling -1 dBFS
  -> per-speaker gain/delay/mute available in Omniphony
        |
PipeWire multichannel output
```

The encoded PipeWire input experiment in Omniphony is intentionally not used. Aurora already owns a direct ALSA/I2S ingress, and `orender` accepts stdin and detects/extracts IEC61937 before calling the bridge. This removes one unproven virtual-sink layer from the runtime.

## Exact external revisions

The authoritative pins live in `config/external-components-v1.json`:

- Omniphony `v0.6.0`, commit `dd5546bbc64e60719dfa367bea0534dc8a3ab34b`
- Harletty bridge `v0.8.0`, commit `eddb123f876048268ee1f096ccdd1cbcf65ad07d`
- VibesboxSRC physical reference commit `8f84376df8b7499808b17c150a328b8665ba1384`
- OpenJOC `0.17.0` remains an independent JOC reference lane rather than a second runtime renderer.

Harletty 0.8.0 and Omniphony 0.6.0 are treated as one compatibility unit because the bridge ABI changed to the 0.4 generation. Do not mix Harletty 0.7.x with Omniphony 0.6.x.

## Why the 0.8 / 0.6 update matters

Harletty 0.8.0 adds fixes relevant to live DD+ JOC: dependent-substream pairing, 7.1 bed preservation, object reconstruction over a 7.1 bed, corrected JOC coefficient/band handling, bed/object alignment, and sparse-object reconstruction. Omniphony 0.6.0 consumes the corresponding bridge ABI and adds format-declared speaker placement.

Aurora validates both the standard 7.1.4 render and its custom 16-output layout. The realtime soak is paced against the Aurora 11.1.4 layout so a passing ARM64 lane means the decoder + object metadata + renderer kept up with media time for the tested fixture.

## Install on Raspberry Pi OS

Install build/runtime prerequisites:

```bash
sudo apt update
sudo apt install -y git build-essential pkg-config python3 alsa-utils \
  libasound2-dev libpipewire-0.3-dev device-tree-compiler
```

Build the exact pinned runtime from source:

```bash
bash scripts/pi5/build-runtime.sh
```

Install the Pi 5 I2S-slave overlay:

```bash
bash scripts/pi5/install-earc-overlay.sh
sudo reboot
```

After reboot:

```bash
arecord -l
arecord -D hw:eARC,0 --dump-hw-params
```

The card must enumerate as `eARC`. Use ALSA card names, not numeric indices.

Start the JOC-capable runtime:

```bash
AURORA_OUTPUT_DEVICE="<PipeWire multichannel device>" \
  bash scripts/pi5/run-earc-joc.sh
```

If `AURORA_OUTPUT_DEVICE` is omitted, Omniphony uses its default PipeWire output.

The launcher defaults to a 48 kHz output graph, an 80 ms PipeWire latency target, and adaptive resampling enabled to absorb long-term capture/output clock drift. These are software defaults, not measured TV-to-speaker latency. They can be tuned without code changes through `AURORA_OUTPUT_RATE`, `AURORA_LATENCY_MS`, and `AURORA_ADAPTIVE_RESAMPLING`. Keep adaptive resampling enabled unless the final hardware demonstrates a shared/locked clock or an equivalent drift-control mechanism.

The default Aurora layout uses Omniphony 0.6's LR4 frequency-band renderer as the 16-channel bass-management stage: the LFE/sub owns 0–80 Hz, the eleven floor speakers start at 80 Hz, and the four height speakers start at 100 Hz. The launcher also applies -3 dB master headroom and enables automatic peak correction with a -1 dBFS ceiling.

A measured-room configuration can be added later without changing the launcher:

```bash
AURORA_RENDER_CONFIG="$HOME/.config/aurora/room.yaml" \
AURORA_OUTPUT_DEVICE="<PipeWire multichannel device>" \
  bash scripts/pi5/run-earc-joc.sh
```

Do not invent room EQ values before measuring the actual speakers and room. The checked-in baseline therefore remains flat apart from crossover/bass routing, gain headroom and anti-clip protection.

## Hardware tap

The overlay is derived from the MIT-licensed VibesboxSRC Pi 5 eARC tap and is stored at `platform/pi5/aurora-earc-tap-overlay.dts`.

The SiI9437 signals are tapped at the chip, before the downstream mode-dependent mux. Keep wires short and use the signal conditioning documented by the Vibesbox reference (approximately 330 ohm series resistance per signal was used in that build). The Pi is clock consumer; the SiI9437 supplies BCLK and WS.

## Validation

Software-only contract:

```bash
bash validation/physical/test-pi5-earc-runtime-contract.sh
```

Full JOC stack:

```bash
AURORA_JOC_BUILD_MODE=release \
AURORA_JOC_REALTIME_LOOPS=4 \
  bash validation/immersive/test-joc-realtime-soak.sh
```

CI runs the JOC stack on native Linux ARM64 and checks the 16-channel paced output.

## Truth boundary

### Demonstrated / CI-testable

- deterministic Pi5 I2S overlay source derived from a hardware-validated reference;
- canonical IEC61937 conversion and clean stdout transport;
- IEC61937 E-AC-3 type `0x15` into Harletty;
- real JOC metadata/object emission from the pinned public fixture;
- Omniphony render into Aurora's 16-output custom geometry;
- 16-output LR4 bass-management topology (80 Hz floor/sub split, 100 Hz height high-pass);
- launcher-level output headroom and anti-clip configuration;
- media-paced JOC render on native ARM64 CI;
- fail-closed pin/version/runtime-contract checks.

### Still requires the physical Aurora unit

- the exact Samsung TV -> Lindy -> this Pi5 wiring under Netflix/Disney+/Prime protected playback;
- sustained Pi5 thermal/CPU headroom for the final chosen DSP configuration;
- actual DAC/PipeWire endpoint latency and xruns;
- end-to-end A/V lip-sync and TV-specific compensation;
- electrical integrity of the soldered tap and final enclosure;
- acoustic calibration and amplifier/speaker validation.

A green software lane means the software path is integrated. It does not turn unmeasured physical behavior into a claim.
