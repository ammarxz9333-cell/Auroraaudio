# Aurora i.MX93 R0 Bring-up

This procedure is ordered so software/interface mistakes are found before power amplifiers can damage a speaker.

## 0. Validate Aurora itself

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --release -p aurora-audio-io --bin aurora-iec61937-extract
```

The IEC61937 extractor is part of `aurora-audio-io` and is covered by the normal Aurora workspace CI.

## 1. Build and pin Harletty + Omniphony

Install the PipeWire development headers first, then:

```bash
./scripts/imx93/build-r0-deps.sh
```

The script verifies the exact R0 Git HEADs and the expected artifacts. Do not benchmark another upstream HEAD and label it R0.

## 2. Run the cross-project JOC integration gate

Before touching i.MX93 hardware:

```bash
./scripts/imx93/validate-joc-714.sh
```

This uses Harletty's committed real one-second E-AC-3 JOC fixture and exercises:

```text
raw E-AC-3 JOC
-> Omniphony stdin Raw transport
-> pinned Harletty bridge
-> pinned Omniphony VBAP
-> Aurora 7.1.4 layout
-> 12ch float32 output
```

The same gate runs in `.github/workflows/imx93-r0-integration.yml`.

## 3. Kernel / device-tree preparation

Apply `platforms/imx93/device-tree/README.md` to the exact byteENGINE/NXP i.MX93 BSP.

Linux must expose:

```text
hw:AuroraEARC,0   capture  S32_LE / 2ch / 192 kHz / external SAI1 clocks
hw:AuroraTDM16,0 playback S32_LE / 16ch / 48 kHz / SAI3 TDM512
```

Use direct `hw:` for encoded input; never `plughw`/software conversion before IEC61937 extraction.

## 4. Install the user PipeWire/runtime layer

With the repository installed at `/opt/aurora` and pinned dependencies at `/opt/aurora-deps` (or matching installer overrides):

```bash
./scripts/imx93/deploy-r0-user.sh
```

This installs:

- `aurora_tdm`: a 16ch S32LE/48k PipeWire sink backed by `hw:AuroraTDM16,0`;
- the per-user Aurora environment;
- a systemd **user** unit sharing the same PipeWire/WirePlumber graph as Omniphony.

It deliberately does not enable/start Aurora.

## 5. Doctor — amplifiers hard-muted

Keep both KAB9 boards unpowered or hard-muted.

```bash
./scripts/imx93/doctor.sh
```

Doctor checks binaries/pins/layout, ALSA inventory, PipeWire node presence and capture-open behavior.

Only while amplifiers are physically hard-muted, also test the 16ch hardware PCM with digital zero:

```bash
AURORA_PROBE_PLAYBACK=1 ./scripts/imx93/doctor.sh
```

## 6. Input electrical test

With an oscilloscope verify after eARC negotiation:

```text
SiI9437 BCLK -> translator -> SAI1_RX_BCLK
SiI9437 WS   -> translator -> SAI1_RX_SYNC
SiI9437 SD0  -> translator -> SAI1_RX_DATA00
```

The DD+ reference path uses a nominal 192 kHz stereo carrier. With 32-bit stereo slots the expected BCLK is about 12.288 MHz; measure it rather than assuming it.

## 7. Capture real Netflix E-AC-3/JOC

Put the TV in eARC pass-through/bitstream mode and play a known Atmos title:

```bash
AURORA_EARC_DEVICE=hw:AuroraEARC,0 \
AURORA_CAPTURE_SECONDS=15 \
./scripts/imx93/capture-joc.sh
```

Artifacts:

```text
*.s32le    raw SAI capture
*.eac3     IEC61937 removed
*.info.txt Harletty report
*.sha256   hashes
```

G3 requires actual `JOC: yes` / OAMD-object evidence. `Pc=0x15` alone proves only E-AC-3.

## 8. i.MX93 real-time performance gate

Use the captured JOC file. Record exact module/BSP/kernel, CPU governor, temperature and clocks. Run at least 30 minutes and require the acceptance margin specified in `VALIDATION_GATES.md` with no deadline/xrun failure.

GitHub/x86 integration success is compatibility evidence, not an A55 performance benchmark.

## 9. DAC-only TDM identification

Still keep amplifiers hard-muted. Configure/play:

```text
48 kHz
S32_LE
16 channels
16 x 32-bit slots = TDM512
```

Use one active slot at a time. Verify the measured analog output against `CHANNEL_MAP.md`:

```text
slots 1..8   -> AK4458 #1
slots 9..16  -> AK4458 #2
```

Upstream Linux/NXP software explicitly supports dual-AK4458 16ch TDM and the AK4458 driver enables daisy-chain mode for >8ch DSP_B/TDM streams, but the exact i.MX93 clock/pin realization remains a physical G6 test.

## 10. Analog/KAB9 safety gate

Using dummy loads or safe test speakers:

- measure DAC full-scale and common-mode;
- freeze analog attenuation/buffer values;
- confirm mute polarity and power-on default;
- check DC, idle noise, clipping and thermal behavior;
- validate any PBTL sub mode independently.

Do not allow the software service to control amplifier unmute until this gate passes.

## 11. Live 7.1.4

After G1-G7:

```bash
systemctl --user enable --now aurora-r0.service
```

or interactively:

```bash
./scripts/imx93/run-live-714.sh
```

Runtime path:

```text
arecord SAI1
-> Aurora IEC61937 extractor
-> raw E-AC-3 JOC
-> Omniphony Raw stdin
-> Harletty JOC/OAMD
-> Omniphony 7.1.4
-> PipeWire aurora_tdm
-> SAI3/dual AK4458 TDM16
```

`run-live-714.sh` fails closed if the exact `aurora_tdm` node is missing; it will not silently fall back to a stereo sink.

## 12. Final two-hour soak

Record source/title/TV settings, all Git/BSP/kernel versions, CPU thermals/clocks, PipeWire/ALSA xruns, renderer/decoder errors, channel identity before/after, and measured A/V offset. Write the acceptance artifact defined by `VALIDATION_GATES.md`.
