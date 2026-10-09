# Live eARC -> Aurora 3D VBAP runtime

Status: integration/runtime proof. Physical eARC and physical 12-channel output remain evidence gates until run on the selected hardware.

## Runtime path

```
TV/player eARC
  -> Linux ALSA S32_LE / 2ch / 192 kHz
  -> canonical IEC61937
  -> E-AC-3 data type 0x15
  -> Harletty object decoder
  -> Aurora LiveImmersiveRuntime
  -> Aurora Vbap3dRenderer
  -> 7.1.4 / 12-channel S32_LE / 48 kHz
  -> one output device or raw file
```

This path deliberately does not use FFmpeg for Atmos decoding and does not use Omniphony as the speaker renderer. Omniphony's `bridge_api` and `spdif` crates remain build-time ABI/parser dependencies for the pinned Harletty bridge.

The runtime is fail-closed for non-`0x15` IEC61937 packets and requires native JOC object evidence before audio is released.

## Software fixture gate

```bash
bash validation/physical/run-live-earc-vbap.sh --fixture
```

The fixture gate uses the checksum-pinned Harletty JOC test stream through the same stdin runtime path, requires non-silent 12-channel output including active height channels, and requires plain E-AC-3 to fail the native-object gate.

## Physical capture to file

First identify the real ALSA capture device with `arecord -l`; never copy a guessed device name.

```bash
bash validation/physical/run-live-earc-vbap.sh \
  --capture-device 'hw:CARD,DEV' \
  --output-file artifacts/live-7.1.4.s32 \
  --seconds 85
```

The runner preserves the canonical IEC61937 capture and runs the strict live-ingress classifier after completion.

## Physical capture to one multichannel ALSA device

```bash
bash validation/physical/run-live-earc-vbap.sh \
  --capture-device 'hw:CARD,DEV' \
  --output-device 'plughw:CARD,DEV' \
  --seconds 85
```

The output contract is interleaved `S32_LE`, 12 channels, 48 kHz in Aurora's canonical 7.1.4 order.

## Truth boundary

A passing fixture CI proves the software connection from IEC61937 JOC through Harletty into Aurora's own 3D renderer. It does not prove a physical eARC capture, a specific streaming service, a specific USB/TDM device, or long-duration dual-clock stability.

The direct ALSA pipeline intentionally has no adaptive sample-rate controller yet. Input eARC and USB output normally have independent clocks, so a long-duration production path still needs the existing Aurora drift/ASRC ownership rules applied at that clock-domain crossing. Do not call this production-ready until the physical xrun/drift gate passes.
