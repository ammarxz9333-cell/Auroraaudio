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
  -> [direct device only] bounded FIFO + PI drift controller + Rubato ASRC
  -> one multichannel ALSA output device

Raw-file capture intentionally bypasses clock adaptation because it is not clocked by a physical sink.
```

This path deliberately does not use FFmpeg for Atmos decoding and does not use Omniphony as the speaker renderer. Omniphony's `bridge_api` and `spdif` crates remain build-time ABI/parser dependencies for the pinned Harletty bridge.

The runtime is fail-closed for non-`0x15` IEC61937 packets and requires native JOC object evidence before audio is released.

## Software fixture gate

```bash
bash validation/physical/run-live-earc-vbap.sh --fixture
```

The fixture gate uses the checksum-pinned Harletty JOC test stream through the same stdin runtime path, requires non-silent 12-channel output, runs the adaptive-output self-test, and requires plain E-AC-3 to fail the native-object gate. The public fixture is not authored to guarantee non-zero programme energy in the four height outputs, so height activity is not an acceptance condition here; 3D height geometry remains covered by the dedicated renderer tests.

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

The output contract is interleaved `S32_LE`, 12 channels, 48 kHz in Aurora's canonical 7.1.4 order. In direct-device mode the rendered stream crosses Aurora's adaptive duplex bridge before `aplay`: a single bounded 12-channel FIFO feeds one shared ASRC ratio, preserving channel coherence while compensating the independent eARC-input and USB-output clocks.


### UMC1820 + ADA8200 output profile

For the proven Linux UMC1820 topology at 48 kHz, the UMC1820 exposes 20 playback slots when the ADAT expansion is present. Aurora provides a dedicated `umc1820-ada8200` sink profile rather than relying on ALSA's default first-N-channel routing.

Logical Aurora 7.1.4 is mapped as follows:

| Aurora | UMC1820 USB playback slot | Physical destination |
| --- | ---: | --- |
| FL | 3 | Line Out 3 |
| FR | 4 | Line Out 4 |
| FC | 5 | Line Out 5 |
| LFE | 6 | Line Out 6 |
| SL | 7 | Line Out 7 |
| SR | 8 | Line Out 8 |
| SBL | 9 | Line Out 9 |
| SBR | 10 | Line Out 10 |
| TFL | 13 | ADAT 1 -> ADA8200 Out 1 |
| TFR | 14 | ADAT 2 -> ADA8200 Out 2 |
| TRL | 15 | ADAT 3 -> ADA8200 Out 3 |
| TRR | 16 | ADAT 4 -> ADA8200 Out 4 |

USB playback slots 1-2, 11-12, and 17-20 are emitted as digital silence. This intentionally avoids the UMC1820 Main 1-2 hardware volume control and avoids sending Aurora channels to S/PDIF.

Use a conversion-capable ALSA endpoint such as `plughw:` for this profile because Linux reports the UMC1820's 20-channel hardware playback format as `S24_3LE`, while Aurora's pipe contract remains `S32_LE`.

```bash
bash validation/physical/run-live-earc-vbap.sh \
  --capture-device 'hw:CAPTURE,DEV' \
  --output-device 'plughw:UMC1820,0' \
  --output-profile umc1820-ada8200 \
  --seconds 85
```

At 48 kHz the optical output carries eight ADAT channels; the profile currently uses the first four and leaves ADAT 5-8 silent. The physical UMC1820 + ADA8200 + Raspberry Pi chain remains an evidence gate until run on the actual hardware.

## Truth boundary

A passing fixture CI proves the software connection from IEC61937 JOC through Harletty into Aurora's own 3D renderer and proves the adaptive-output bridge builds and passes its synthetic multichannel self-test. It does not prove a physical eARC capture, a specific streaming service, a specific USB/TDM device, or long-duration dual-clock stability.

The direct ALSA pipeline now wires Aurora's existing adaptive drift/ASRC path at the clock-domain crossing. That removes the previous architectural gap, but it is still software evidence only until the real Pi/eARC/input clock and the selected multichannel USB output clock complete a long-duration physical run without XRUNs, uncontrolled fill excursions, adaptive faults, or audible artifacts.
