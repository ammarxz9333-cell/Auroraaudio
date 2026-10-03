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
raw interleaved F32, 16ch @ 48 kHz
        |
CamillaDSP 4.1.3
  -> optional PEQ/FIR/measured room correction
  -> AsyncSinc rate-adjust for independent eARC/DAC clocks
  -> ALSA 16-channel hardware output
```

The encoded PipeWire input experiment in Omniphony is intentionally not used. Aurora owns the direct ALSA/I2S ingress, and `orender` accepts IEC61937 on stdin. The selected output path also avoids a virtual PipeWire handoff: Omniphony emits raw 16-channel F32 directly to CamillaDSP over a Unix pipe, and CamillaDSP writes the physical multichannel DAC through ALSA. PipeWire remains a diagnostic fallback only.

## Exact external revisions

The authoritative pins live in `config/external-components-v1.json`:

- Omniphony `v0.6.0`, commit `dd5546bbc64e60719dfa367bea0534dc8a3ab34b`
- Harletty bridge `v0.8.0`, commit `eddb123f876048268ee1f096ccdd1cbcf65ad07d`
- VibesboxSRC physical reference commit `8f84376df8b7499808b17c150a328b8665ba1384`
- CamillaDSP `4.1.3`, commit `05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74`; the official Linux ARM64 archive is SHA-256 pinned in the external-component manifest
- OpenJOC `0.17.0` remains an independent JOC reference lane rather than a second runtime renderer.

Harletty 0.8.0 and Omniphony 0.6.0 are treated as one compatibility unit because the bridge ABI changed to the 0.4 generation. Do not mix Harletty 0.7.x with Omniphony 0.6.x.

## Why the 0.8 / 0.6 update matters

Harletty 0.8.0 adds fixes relevant to live DD+ JOC: dependent-substream pairing, 7.1 bed preservation, object reconstruction over a 7.1 bed, corrected JOC coefficient/band handling, bed/object alignment, and sparse-object reconstruction. Omniphony 0.6.0 consumes the corresponding bridge ABI and adds format-declared speaker placement.

Aurora validates both the standard 7.1.4 render and its custom 16-output layout. The realtime soak is paced against the Aurora 11.1.4 layout so a passing ARM64 lane means the decoder + object metadata + renderer kept up with media time for the tested fixture.

## Install on Raspberry Pi OS

Install build/runtime prerequisites:

```bash
sudo apt update
sudo apt install -y git build-essential pkg-config python3 alsa-utils curl \
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

Start the JOC-capable runtime after selecting the physical 16-channel ALSA device:

```bash
AURORA_ALSA_OUTPUT_DEVICE="hw:<your-16ch-device>" \
  bash scripts/pi5/run-earc-joc.sh
```

The default output mode is `camilladsp`. Aurora generates a flat 16-channel CamillaDSP configuration with 48 kHz F32 stdin, ALSA playback, a 512-frame processing chunk, target playback level 512 samples, and `AsyncSinc` rate adjustment enabled. The rate servo is important because the TV/eARC capture clock and the DAC playback clock are independent. The exact optimal chunk/target values remain a physical tuning gate; they are configurable through `AURORA_CAMILLADSP_CHUNK`, `AURORA_CAMILLADSP_TARGET_LEVEL`, `AURORA_CAMILLADSP_QUEUELIMIT`, and `AURORA_CAMILLADSP_ADJUST_PERIOD`.

The default Aurora layout uses Omniphony 0.6's LR4 frequency-band renderer as the bass-management stage: the sub owns 0–80 Hz, the eleven floor speakers start at 80 Hz, and the four height speakers start at 100 Hz. The launcher applies -3 dB master headroom and enables automatic peak correction with a -1 dBFS ceiling before handing F32 PCM to CamillaDSP.

The generated CamillaDSP baseline is deliberately **flat**: it proves the realtime 16-channel post-DSP boundary and clock servo without inventing room correction. After measuring the actual speakers and room, provide a full CamillaDSP configuration with the same 16-channel F32 stdin contract:

```bash
AURORA_CAMILLADSP_CONFIG="$HOME/.config/aurora/room-correction.yml" \
AURORA_ALSA_OUTPUT_DEVICE="hw:<your-16ch-device>" \
  bash scripts/pi5/run-earc-joc.sh
```

That deployment config may add per-channel PEQ, FIR convolution, delays, trims and other CamillaDSP processing. Acoustic improvement remains physical/measured evidence, not a software-only claim.

`AURORA_RENDER_CONFIG` is separate: it is an optional Omniphony renderer configuration, not the room-EQ file.

For diagnostics only, the previous direct PipeWire output remains available:

```bash
AURORA_OUTPUT_MODE=pipewire \
AURORA_OUTPUT_DEVICE="<PipeWire multichannel device>" \
  bash scripts/pi5/run-earc-joc.sh
```

For daily use, install the optional systemd user service after the runtime and overlay are ready:

```bash
bash scripts/pi5/install-user-service.sh
# or install and start immediately:
bash scripts/pi5/install-user-service.sh --start
```

The installer creates `~/.config/aurora/runtime.env`, defaults to CamillaDSP, and enables `aurora-earc.service` for the user session. Set `AURORA_ALSA_OUTPUT_DEVICE` there before first real playback. Before starting playback, or when diagnosing a failure, run:

```bash
bash scripts/pi5/check-health.sh
```

The health check verifies the installed renderer, Harletty bridge, CamillaDSP binary, layout, eARC ALSA endpoint and latest ingress status, plus Pi temperature/throttling data when available.

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

## Pi 5 RP1 multichannel playback warning

Do **not** treat RP1 four-lane I2S playback as Aurora's guaranteed final 8/16-channel output path yet.

Raspberry Pi Linux issue #7584 documents a Pi 5 + four-lane I2S + CamillaDSP case where, after an XRUN recovery, ALSA and CamillaDSP can remain RUNNING while physical output pairs are missing or remapped. Raspberry Pi PR #7588 restores the stronger upstream DesignWare I2S stop teardown and initial hardware checks on the reporter's DAC8x system were good, but the recovery-specific XRUN stress test was still outstanding when this profile was updated.

This issue concerns **playback/TX recovery**, not Aurora's Lindy/SiI9437 eARC **capture/RX** path. Aurora therefore keeps the Pi5 eARC input design, but the final 16-channel playback endpoint should use a separately validated ALSA multichannel device (for example USB/ADAT/other interface) until the RP1 playback fix is merged and recovery-tested.

References:
- https://github.com/raspberrypi/linux/issues/7584
- https://github.com/raspberrypi/linux/pull/7588

## Truth boundary

### Demonstrated / CI-testable

- deterministic Pi5 I2S overlay source derived from a hardware-validated reference;
- canonical IEC61937 conversion and clean stdout transport;
- IEC61937 E-AC-3 type `0x15` into Harletty;
- real JOC metadata/object emission from the pinned public fixture;
- Omniphony render into Aurora's 16-output custom geometry;
- 16-output LR4 bass-management topology (80 Hz floor/sub split, 100 Hz height high-pass);
- launcher-level output headroom and anti-clip configuration;
- generated 16-channel CamillaDSP contract with F32 stdin, ALSA playback and AsyncSinc rate adjustment;
- checksum-pinned official CamillaDSP Linux ARM64 runtime artifact;
- media-paced JOC render on native ARM64 CI;
- fail-closed pin/version/runtime-contract checks.

### Still requires the physical Aurora unit

- the exact Samsung TV -> Lindy -> this Pi5 wiring under Netflix/Disney+/Prime protected playback;
- sustained Pi5 thermal/CPU headroom for the final chosen DSP configuration;
- actual DAC/ALSA endpoint latency, rate-adjust behaviour and xruns;
- end-to-end A/V lip-sync and TV-specific compensation;
- electrical integrity of the soldered tap and final enclosure;
- acoustic calibration and amplifier/speaker validation.

A green software lane means the software path is integrated. It does not turn unmeasured physical behavior into a claim.
