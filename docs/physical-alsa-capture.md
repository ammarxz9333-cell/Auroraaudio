# Physical ALSA encoded-ingress capture

Status: capture/conversion tooling for issue #143. This is not physical evidence until it is run on the selected Lindy 38368 / SiI9437 -> Linux I2S capture hardware.

## Purpose

`validation/physical/aurora_alsa_iec61937_capture.py` bridges a Linux ALSA capture into the canonical IEC61937 byte stream consumed by `aurora_physical_ingress.py`.

It intentionally does **not** hard-code a Raspberry Pi ALSA device name. The operator must identify and pass the actual capture PCM device after the physical I2S slave/capture adapter is configured.

The first encoded-ingress format to probe is:

- `S32_LE`
- 2 channels
- 192000 Hz

This reflects Aurora's encoded eARC/IEC61937 transport requirement and older project validation planning, but it is not proof that an arbitrary Pi driver exposes the SiI9437 tap in this exact ALSA shape. Always inspect the real device's hardware parameters first.

## One capture run

First list/identify the actual ALSA capture device on the host:

```bash
arecord -l
```

Then run the adapter with the explicit device name; do not copy the example device string blindly:

```bash
python3 validation/physical/aurora_alsa_iec61937_capture.py capture \
  --device 'hw:CARD,DEV' \
  --seconds 85 \
  --raw-out artifacts/physical/alsa-s32le-2ch-192k.raw \
  --iec-out artifacts/physical/earc-joc.spdif \
  --metadata artifacts/physical/capture-metadata.json \
  --conversion-report artifacts/physical/conversion-report.json \
  --hw-params-log artifacts/physical/arecord-hw-params.log \
  --stderr-log artifacts/physical/arecord-stderr.log
```

The capture command:

1. probes the selected PCM with the requested `S32_LE / 2ch / 192 kHz` parameters;
2. records start/end monotonic timestamps around the real `arecord` process;
3. preserves the raw ALSA capture;
4. counts `xrun`/`overrun`/`underrun` text markers from `arecord` as diagnostics;
5. converts the 32-bit ALSA sample slots into a canonical 16-bit IEC byte stream;
6. auto-detects only the explicit supported interpretations: high-16 vs low-16 sample placement and LR vs RL channel order;
7. requires a real fixed-grid IEC61937 burst train before selecting an interpretation.

It does not guess arbitrary bit shifts and it does not infer hardware reset/drop counters from silence or from process exit status.

## Hardware reset/drop counters

Physical Gate A requires explicit reset/drop evidence. `arecord` stderr is not enough to prove a hardware reset counter is zero.

If the real capture driver, kernel instrumentation, FPGA/MCU adapter, or other trustworthy physical capture source provides cumulative reset/drop counts, pass them to the capture command with their provenance:

```bash
  --hardware-reset-count 0 \
  --hardware-drop-count 0 \
  --hardware-counter-source 'describe the real counter source here'
```

If those counters are unavailable, leave them omitted. The capture can still be preserved and converted for debugging, but it must **not** be promoted to a completed Gate A result.

## Run Gate A on the canonical stream

After a clean capture, use the recorded monotonic timestamps and the real hardware reset/drop counters with the merged physical ingress validator:

```bash
python3 validation/physical/aurora_physical_ingress.py analyze \
  --capture artifacts/physical/earc-joc.spdif \
  --report artifacts/physical/ingress-report.json \
  --write-payload artifacts/physical/reconstructed.ec3 \
  --expected-bursts 2360 \
  --expected-payload-sha256 0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0 \
  --capture-start-monotonic-ns '<from capture-metadata.json>' \
  --capture-end-monotonic-ns '<from capture-metadata.json>' \
  --capture-reset-count 0 \
  --capture-drop-count 0 \
  --require-capture-metadata
```

A valid first physical carrier must reconstruct to the pinned SHA-256 above and preserve all 2360 E-AC-3 `0x15` bursts on the 24576-byte burst grid.

## Existing raw capture conversion

If the hardware capture already exists as `S32_LE / 2ch / 192 kHz` raw ALSA bytes, conversion can be run without new hardware I/O:

```bash
python3 validation/physical/aurora_alsa_iec61937_capture.py convert \
  --input capture.raw \
  --output earc-joc.spdif \
  --report conversion-report.json
```

## Tooling self-test

```bash
python3 validation/physical/aurora_alsa_iec61937_capture.py self-test
```

The self-test round-trips all four explicitly supported ALSA interpretations (`high16/low16` x `LR/RL`) and requires sync-free input to fail closed. CI success validates conversion logic only; it is not evidence that the physical Pi/I2S setup works.

## Truth boundary

This adapter deliberately separates three claims:

- **capture adapter works:** software/tooling evidence;
- **real IEC61937 capture is exact:** Gate A physical ingress evidence after the real capture passes `aurora_physical_ingress.py` with real counter metadata;
- **Aurora produces physical synchronous 7.1.4:** later Gates B-D of issue #143.

Never merge those claims into one result.
