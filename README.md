# Aurora i.MX93 R0

Aurora R0 is a **private DIY 7.1.4 home-theater audio appliance**. The selected R0 architecture receives the TV's legitimate HDMI eARC audio return, preserves Dolby Digital Plus / E-AC-3 JOC as an encoded stream, decodes object audio with Harletty, renders it to a physical 7.1.4 loudspeaker layout with Omniphony, and drives a native 16-channel DAC/amplifier path from an NXP i.MX93 module.

This branch is the hardware-integration branch: `aurora-imx93-r0`. The older `main-v2` software platform and its tests are retained as the software foundation and historical reference; the old N100/STM32H753, Raspberry Pi, CM5, MCHStreamer and ADAU1466 chains are no longer the selected R0 hardware architecture.

> **Capability rule:** a component can be selected because its interfaces are documented, but Aurora does not call the full appliance proven until the matching physical validation gate has passed. In particular, Netflix DD+ JOC preservation, Harletty real-time performance on the exact i.MX93 module, and native 12-channel playback remain hardware gates until measured.

## Selected R0 architecture

```text
Netflix / TV box / streaming app
            │ HDMI / HDCP
            ▼
            TV
            │ eARC audio return
            ▼
Lindy 38368 prototype / SiI9437 eARC receiver
            │
            │ BCLK + LRCLK + SD0
            │ IEC 61937 DD+ carrier, 192 kHz
            ▼
SN74LVC3G17 3.3 V → 1.8 V
            │
            ▼
byteENGINE i.MX93 OSM-S
  ├─ SAI1 RX slave: S32_LE / 2 ch / 192 kHz ingress
  ├─ aurora-iec61937-extract: IEC 61937 → raw E-AC-3
  ├─ Harletty bridge: E-AC-3 JOC → PCM + OAMD
  ├─ Omniphony: objects/OAMD → 7.1.4 speaker render
  └─ SAI3 TX master: 48 kHz / 32-bit / TDM512 / 16 slots
            │
            ▼
SN74AXC4T245 1.8 V → 3.3 V
            │ MCLK + BCLK + LRCLK + DATA
            ▼
AK4458 #2 ──TDMO1──> AK4458 #1
            │          │
            └── 16 differential DAC outputs ──┐
                                               ▼
                                analog LPF / attenuation
                                               │
                                               ▼
                               2 × WONDOM KAB9
                               12 channels used
                                               │
                                               ▼
                                            7.1.4
```

### Why this path

- The TV remains the legitimate HDCP endpoint; Aurora does not capture protected HDMI video.
- Lattice SiI9437 is an eARC receiver intended for soundbar/AVR use and exposes S/PDIF / multichannel I2S audio outputs.
- A real Lindy 38368 / SiI9437 system has already been measured carrying E-AC-3 as IEC 61937 at a 192 kHz carrier on the I2S tap.
- byteENGINE i.MX93 OSM-S exposes SAI1 RX pins suitable for the eARC tap and a separate SAI3 TX path for native multichannel output.
- AK4458 explicitly supports TDM512 and daisy chaining two devices for 16-channel playback from one serial data stream.
- Two KAB9 boards provide enough differential-input amplifier channels for 7.1.4 while leaving reserve channels.

See [`docs/imx93-r0/ARCHITECTURE.md`](docs/imx93-r0/ARCHITECTURE.md) and [`docs/imx93-r0/HARDWARE.md`](docs/imx93-r0/HARDWARE.md) for the engineering detail.

## Evidence status

| Segment | Status |
| --- | --- |
| TV eARC → SiI9437 / Lindy | externally hardware-validated |
| SiI9437 → IEC 61937 E-AC-3 @ 192 kHz | externally hardware-validated |
| Aurora IEC 61937 deframer | implemented with deterministic unit tests on this branch |
| Harletty E-AC-3 JOC decode | implemented upstream; current R0 pin is `4ccedec` |
| Omniphony 7.1.4 speaker render | implemented upstream; current R0 pin is `44acc87` |
| i.MX93 SAI1 physical eARC capture | **R0 hardware gate** |
| Netflix Atmos title → JOC detected on i.MX93 capture | **R0 hardware gate** |
| Harletty worst-case JOC real-time on i.MX93 A55 | **R0 performance gate** |
| i.MX93 SAI3 → dual AK4458 TDM512 | **R0 hardware gate** |
| 12-channel DAC → KAB9 → speakers | **R0 hardware/analog gate** |
| full Netflix → 7.1.4 continuous playback | **final R0 acceptance gate** |

The gate definitions are in [`docs/imx93-r0/VALIDATION_GATES.md`](docs/imx93-r0/VALIDATION_GATES.md).

## New R0 software ingress

The existing `aurora-audio-io` package now also contains the R0 IEC 61937 extractor binary:

```bash
cargo build --release -p aurora-audio-io --bin aurora-iec61937-extract
```

For the SiI9437 I2S tap representation:

```bash
arecord -D hw:AuroraEARC,0 -f S32_LE -c 2 -r 192000 -t raw \
  | ./target/release/aurora-iec61937-extract --width s32 --codec eac3 \
  > capture.eac3
```

The extractor understands the important DD+ detail that IEC 61937 data type `0x15` expresses `Pd` in **bytes**, while AC-3/DTS use a bit count. It also strips the unused lower half of each SiI9437 S32 sample and restores native E-AC-3 byte order.

## Third-party runtime pins

R0 deliberately pins known source states rather than silently following upstream changes:

```text
Harletty bridge : 4ccedec804de3b29c02fb2a69575c2f49bf2fb37
Omniphony       : 44acc87a9cbf4b5ac8f474f51d87851d2c642550
```

These are source dependencies for the private appliance and are not vendored into Aurora. See [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).

## Bring-up order

Do not connect speakers and try Netflix first. R0 is validated in layers:

1. software IEC 61937 tests;
2. SAI1 clock/data capture with the amplifier muted;
3. capture and identify `Pa/Pb/Pc=0x15`;
4. prove the captured elementary stream contains JOC/object metadata;
5. benchmark Harletty on the exact i.MX93 hardware;
6. validate 16-slot TDM512 into the two AK4458 DACs with test tones;
7. validate channel order, level and mute sequencing into the KAB9 boards;
8. only then perform continuous Netflix → 7.1.4 playback and lip-sync measurements.

Detailed commands are in [`docs/imx93-r0/BRINGUP.md`](docs/imx93-r0/BRINGUP.md).

## Existing Aurora software

The repository still contains the prior Rust scene, renderer, DSP, real-time, simulation, diagnostics and CLI work. It is useful for test infrastructure and future Aurora-owned DSP/control, but it must not be confused with evidence for the selected third-party JOC renderer path.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

## Scope

R0 is for one private, non-commercial DIY system. It is not described as Dolby-certified, HDMI-certified, or a commercial Atmos product. If the project later changes scope to distribution or sale, codec/patent/trademark, GPL redistribution and product-compliance obligations must be reviewed separately.
