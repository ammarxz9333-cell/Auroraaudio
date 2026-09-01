# Aurora i.MX93 R0 Bring-up

This procedure is intentionally ordered so a bad clock, channel map or analog gain setting is found before power amplifiers can damage a speaker.

## 0. Build the existing Aurora workspace

On a development machine:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --release -p aurora-audio-io --bin aurora-iec61937-extract
```

The R0 extractor is part of the existing `aurora-audio-io` package, so no new dependency or lockfile update is required.

## 1. Build pinned Harletty and Omniphony

```bash
./scripts/imx93/build-r0-deps.sh
```

The script checks out the exact R0 commits. Do not benchmark a different upstream HEAD and label the result as R0 evidence.

## 2. Kernel / device-tree preparation

Start from the byteENGINE i.MX93 BSP device tree and apply the requirements in:

```text
platforms/imx93/device-tree/README.md
```

At minimum Linux must expose:

- a capture PCM backed by SAI1 RX, 2 channels, S32_LE, 192 kHz, externally clocked;
- a playback PCM backed by SAI3 and the two AK4458 codecs, supporting 16 channels / 32-bit TDM at 48 kHz.

Do not use `plughw` for the encoded input.

## 3. Input electrical test — no amplifiers

Keep both KAB9 boards unpowered or hard-muted.

Check:

```bash
./scripts/imx93/doctor.sh
```

With an oscilloscope verify, after eARC negotiation:

```text
SiI9437 BCLK → translator → SAI1_RX_BCLK
SiI9437 WS   → translator → SAI1_RX_SYNC
SiI9437 SD0  → translator → SAI1_RX_DATA00
```

For DD+ the reference hardware measured a 192 kHz stereo carrier. With 32-bit stereo framing expect BCLK around 12.288 MHz. Measure instead of assuming.

## 4. Capture E-AC-3 from the TV

Set the TV's digital/eARC audio mode to pass-through / bitstream as appropriate for that TV. Play a known Atmos title.

```bash
AURORA_EARC_DEVICE=hw:AuroraEARC,0 \
AURORA_CAPTURE_SECONDS=15 \
./scripts/imx93/capture-joc.sh
```

The script creates:

```text
*.s32le    raw SAI capture
*.eac3     IEC61937 wrapper removed
*.info.txt Harletty stream report
*.sha256   exact artifact hashes
```

Required G3 evidence in the info report:

```text
Codec : EAC3 (Dolby Digital Plus)
JOC   : yes
```

`Pc=0x15` alone is not enough; it only proves E-AC-3.

## 5. Performance gate

Use the exact captured `.eac3` file, not a synthetic stream.

Example measurement:

```bash
/usr/bin/time -v "$AURORA_HARLETTY_CLI" \
  --codec eac3 --loglevel error decode capture.eac3
```

Calculate:

```text
RTF = (user CPU seconds + system CPU seconds attributable to decode/render as defined by the test) / audio duration seconds
```

For the appliance gate, collect a 30-minute run and require average RTF below 0.80 with no deadline failure. Keep the CPU governor and thermal conditions in the report.

## 6. DAC-only TDM test

Do not start object decoding yet. Configure SAI3 for:

```text
48 kHz
16 channels
32-bit slots
TDM512
```

Configure both AK4458 devices for the same PCM format and the documented two-device TDM512 daisy chain.

Generate a 16-channel test where only one slot is active at a time. Confirm:

- slots 1..8 appear at AK4458 #1 outputs;
- slots 9..16 appear at AK4458 #2 outputs;
- no output appears on two DAC channels simultaneously;
- clocks remain phase-stable over a 10-minute run.

Only after this test should the analog stage be connected to the power amps.

## 7. Analog gain and KAB9 test

Use dummy loads / safe test speakers first.

Measure the DAC output and amplifier input at low software level. Determine the analog attenuation needed so a full-scale digital error cannot grossly overdrive the selected KAB9 gain setting.

Then identify all twelve channels individually at low level.

## 8. Live 7.1.4 pipeline

After G1–G7 are satisfied:

```bash
AURORA_EARC_DEVICE=hw:AuroraEARC,0 \
AURORA_OUTPUT_DEVICE=aurora_tdm \
./scripts/imx93/run-live-714.sh
```

This path is:

```text
arecord SAI1
→ Aurora IEC61937 extractor
→ raw E-AC-3 JOC
→ orender + Harletty bridge
→ Omniphony 7.1.4
→ PipeWire playback device for SAI3/AK4458
```

## 9. Final soak

Record:

- title/source/TV model and TV audio setting;
- all commit hashes;
- BSP/kernel version;
- CPU temperatures and clock;
- ALSA/PipeWire xruns;
- Harletty/renderer errors;
- channel identity before and after run;
- measured audio/video offset.

Run for two hours. If it passes, write the acceptance artifact required by `VALIDATION_GATES.md`.
