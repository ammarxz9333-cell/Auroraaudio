# Aurora i.MX93 R0

Aurora R0 is a **private DIY 7.1.4 home-theater audio appliance**. The selected R0 architecture receives the TV's legitimate HDMI eARC audio return, preserves Dolby Digital Plus / E-AC-3 JOC as an encoded stream, decodes object audio with Harletty, renders it to a physical 7.1.4 loudspeaker layout with Omniphony, and drives a native 16-channel DAC/amplifier path from an NXP i.MX93 module.

This branch is the hardware-integration branch: `aurora-imx93-r0`. The older `main-v2` software platform and its tests are retained as the software foundation and historical reference; the old N100/STM32H753, Raspberry Pi, CM5, MCHStreamer and ADAU1466 chains are no longer the selected R0 hardware architecture.

> **Capability rule:** a component can be selected because its interfaces are documented, but Aurora does not call the full appliance proven until the matching physical validation gate has passed. In particular, Netflix DD+ JOC preservation, Harletty real-time performance on the exact i.MX93 module, and native 16-channel TDM playback remain hardware gates until measured.

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
  └─ SAI3 TX master: 48 kHz / S32_LE / TDM512 / 16 slots
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
- upstream Linux/NXP contains an explicit dual-AK4458 16-channel TDM path; the codec driver supports DSP_B/TDM/S32_LE and daisy-chain operation above eight channels.
- Two KAB9 boards provide enough differential-input amplifier channels for 7.1.4 while leaving reserve channels.

Engineering references:

- [`docs/imx93-r0/ARCHITECTURE.md`](docs/imx93-r0/ARCHITECTURE.md)
- [`docs/imx93-r0/SOFTWARE_AUDIT.md`](docs/imx93-r0/SOFTWARE_AUDIT.md)
- [`docs/imx93-r0/CHANNEL_MAP.md`](docs/imx93-r0/CHANNEL_MAP.md)
- [`docs/imx93-r0/HARDWARE.md`](docs/imx93-r0/HARDWARE.md)
- [`docs/imx93-r0/BRINGUP.md`](docs/imx93-r0/BRINGUP.md)

## Evidence status

| Segment | Status |
| --- | --- |
| TV eARC → SiI9437 / Lindy | externally hardware-validated |
| SiI9437 → IEC 61937 E-AC-3 @ 192 kHz | externally hardware-validated |
| Aurora IEC 61937 deframer | implemented + deterministic workspace tests |
| raw E-AC-3 stdin → Harletty bridge | code-audited against pinned Omniphony/Harletty |
| Harletty E-AC-3 JOC decode | implemented upstream; R0 pin `4ccedec` |
| Omniphony 7.1.4 speaker render | implemented upstream; R0 pin `44acc87` |
| real Harletty-JOC → Omniphony-7.1.4 compatibility | dedicated R0 GitHub integration workflow |
| PipeWire 7.1.4 → 16ch TDM sink contract | implemented in `platforms/imx93` |
| Linux dual AK4458 / 16ch TDM software model | explicitly supported upstream |
| i.MX93 SAI1 physical eARC capture | **R0 hardware gate** |
| Netflix Atmos title → JOC detected on i.MX93 capture | **R0 hardware gate** |
| Harletty worst-case JOC real-time on i.MX93 A55 | **R0 performance gate** |
| i.MX93 SAI3 → dual AK4458 TDM512 | **R0 hardware/BSP gate** |
| DAC → KAB9 analog safety | **R0 hardware/analog gate** |
| full Netflix → 7.1.4 continuous playback | **final R0 acceptance gate** |

The gate definitions are in [`docs/imx93-r0/VALIDATION_GATES.md`](docs/imx93-r0/VALIDATION_GATES.md).

## Software verification

Aurora's normal CI verifies the workspace on Linux stable, Windows stable and Rust 1.78 MSRV. The R0 branch adds a second integration layer:

```bash
./scripts/imx93/build-r0-deps.sh
./scripts/imx93/validate-joc-714.sh
```

The integration test uses Harletty's committed real E-AC-3 JOC fixture and sends it through the actual pinned Harletty bridge + pinned Omniphony renderer + Aurora 7.1.4 layout, then validates the resulting 12-channel float stream. See `SOFTWARE_AUDIT.md` for exact scope and limitations.

## New R0 software ingress

Build:

```bash
cargo build --release -p aurora-audio-io --bin aurora-iec61937-extract
```

Capture/unwrap example:

```bash
arecord -D hw:AuroraEARC,0 -f S32_LE -c 2 -r 192000 -t raw \
  | ./target/release/aurora-iec61937-extract --width s32 --codec eac3 \
  > capture.eac3
```

The extractor handles the critical E-AC-3 IEC61937 rule that data type `0x15` expresses `Pd` in **bytes**, while AC-3/DTS use a bit count. It also extracts the useful high word from the SiI9437 S32 capture representation and restores native E-AC-3 byte order.

## Runtime deployment

The selected Linux runtime is a per-user PipeWire graph, not a root/system PipeWire session.

```bash
./scripts/imx93/deploy-r0-user.sh
./scripts/imx93/doctor.sh
```

Deployment installs the canonical `aurora_tdm` 16-channel PipeWire sink, Aurora environment file and systemd **user** service. It deliberately does not start Aurora until the physical safety gates are complete.

Canonical logical order:

```text
FL FR C LFE BL BR SL SR TFL TFR TRL TRR
```

The physical TDM bus remains 16 channels; the final four slots are reserves. See `CHANNEL_MAP.md`.

## Third-party runtime pins

```text
Harletty bridge : 4ccedec804de3b29c02fb2a69575c2f49bf2fb37
Omniphony       : 44acc87a9cbf4b5ac8f474f51d87851d2c642550
```

`build-r0-deps.sh` verifies the exact Git HEADs and expected build artifacts. These are source dependencies for the private appliance and are not vendored into Aurora. See [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).

## Bring-up order

1. Aurora workspace CI + IEC61937 tests.
2. Real JOC → Harletty → Omniphony 7.1.4 software integration gate.
3. i.MX93 device tree and stable ALSA IDs.
4. user PipeWire `aurora_tdm` deployment.
5. SAI1 electrical/capture validation with amplifiers hard-muted.
6. real Netflix capture proving JOC/OAMD survives.
7. i.MX93 real-time A55/thermal benchmark.
8. SAI3 16-slot TDM512 + dual-AK4458 channel identification.
9. analog/KAB9 gain, mute and noise safety.
10. two-hour live Netflix → 7.1.4 soak + A/V sync measurement.
