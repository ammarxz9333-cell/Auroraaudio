# Aurora i.MX93 R0 Architecture

## Objective

R0 is a single private appliance that receives encoded streaming immersive audio from a TV over eARC and produces twelve synchronized analog loudspeaker channels for a physical 7.1.4 layout.

The architecture is intentionally split into four clock/security domains:

1. HDMI/HDCP terminates at the TV.
2. eARC audio transport terminates at the SiI9437 receiver.
3. encoded audio decode/render runs on the i.MX93 Cortex-A55 cores.
4. 48 kHz speaker playback is generated on an independent i.MX93 SAI3 clock domain.

This removes the need for Aurora to be an HDMI video receiver or to possess HDCP receiver keys.

## Signal chain

```text
Streaming source
  │ HDMI
  ▼
TV: HDCP endpoint + eARC source
  │
  │ eARC
  ▼
Lattice SiI9437 in Lindy 38368
  │
  │ I2S-style encoded output
  │ BCLK / WS / SD0
  ▼
SN74LVC3G17, VCC=1.8 V
  │
  ▼
i.MX93 SAI1 RX slave
  │ S32_LE, stereo carrier, 192 kHz for DD+
  ▼
aurora-iec61937-extract
  │ raw E-AC-3 access units
  ▼
Harletty bridge
  │ PCM + OAMD/object metadata
  ▼
Omniphony
  │ 12-channel 7.1.4 PCM @48 kHz
  ▼
i.MX93 SAI3 TX master
  │ TDM512: 16 slots × 32 bits × 48 kHz
  ▼
SN74AXC4T245, 1.8 V → 3.3 V
  │
  ▼
AK4458 #2 (16-slot input)
  ├─ local DAC channels: slots 9..16
  └─ TDMO1: shifted first 8 channels
        │
        ▼
AK4458 #1
  └─ local DAC channels: slots 1..8

16 differential analog outputs
  │ 12 used / 4 reserve
  ▼
analog LPF + level conditioning
  ▼
2 × KAB9
  ▼
7.1.4 speakers
```

## Ingress representation

The reference SiI9437 implementation has been measured on hardware with E-AC-3/DD+ as an IEC 61937 stream carried on SD0 at a nominal 192 kHz stereo carrier. The capture interface presents 24-bit left-justified audio words in S32_LE slots; for the encoded stream, the useful 16-bit IEC word is the upper half of each 32-bit sample.

Aurora therefore performs two reversible transport operations before decode:

1. keep bytes 2..3 from each S32_LE sample;
2. parse IEC 61937 `Pa=0xF872`, `Pb=0x4E1F`, `Pc`, `Pd`; for E-AC-3 `Pc & 0x1f = 0x15` and `Pd` is a byte count; restore each payload word's native byte order.

No lossy decode or PCM conversion occurs before Harletty.

## Decoder and renderer

Harletty is kept outside Aurora's crate graph. The R0 validation pin is:

```text
4ccedec804de3b29c02fb2a69575c2f49bf2fb37
```

That revision includes the late-August E-AC-3/JOC fixes selected during Aurora research. Harletty's bridge translates E-AC-3 JOC to PCM plus OAMD-shaped object metadata.

Omniphony is also external. R0 pins:

```text
44acc87a9cbf4b5ac8f474f51d87851d2c642550
```

R0 uses Omniphony's physical speaker path and the stock `7.1.4` layout rather than binaural convolution.

## CPU allocation

Do not assume multi-core acceleration inside the JOC hot loop. Harletty's current JOC reconstruction is substantially single-threaded. The R0 runtime therefore treats one A55 core as decode/render-critical and keeps background work light. Actual acceptance is based on measured real-time factor, not a synthetic CPU score.

Target performance gate:

```text
worst observed DD+ JOC average RTF < 0.80
no deadline misses in a 30 minute decode/render run
```

The second A55 core remains available for PipeWire/ALSA orchestration, control, telemetry and non-hot-path DSP. The Cortex-M33 is not required for R0 correctness; it remains an optional future hard-real-time control/I/O resource.

## Output transport

R0 separates input and output serial interfaces:

- **SAI1 RX** is a clock slave to the SiI9437 eARC receiver and may operate at the 192 kHz DD+ carrier.
- **SAI3 TX** is Aurora's playback clock master at 48 kHz.

This avoids coupling the encoded input carrier clock to the speaker DAC clock.

For 16 × 32-bit slots at 48 kHz:

```text
BCLK = 48,000 × 16 × 32 = 24.576 MHz
```

AK4458 TDM512 is explicitly defined around a 512fs BICK, so this framing maps naturally to its 16-channel serial mode.

## DAC topology

AKM documents two-AK4458 daisy chain operation in TDM512 mode. The DSP feeds sixteen channels into the second device's SDTI1. The second device consumes the later eight channels and emits the first eight on TDMO1; TDMO1 feeds SDTI1 of the first device. Both devices share LRCK/BICK and compatible data-format settings.

This is preferable to running two independent 8-channel DAC buses because all sixteen output channels share one playback clock and one serial frame.

## Amplifier mapping

R0 assigns twelve DAC outputs as:

```text
1  FL       5  SL       9  Top Front Left
2  FR       6  SR      10  Top Front Right
3  C        7  BL      11  Top Rear Left
4  LFE      8  BR      12  Top Rear Right
13..16 reserve / test
```

A practical KAB9 mapping is:

- KAB9-A: seven bed speakers plus one reserve channel, 8 × BTL.
- KAB9-B: four height channels in BTL plus one PBTL pair for the subwoofer; remaining channels are reserve. If the subwoofer uses its own powered amplifier, KAB9-B can remain entirely BTL.

The final analog attenuation/LPF values are a hardware gate. AK4458 full-scale differential output is materially higher than the KAB9 input required for full output at its gain settings, so the production prototype must not connect full-scale DAC outputs blindly to the power amp.

## Failure behavior

The default failure state is mute. Amp enable must occur only after:

- SAI3 clock is stable;
- both DACs are configured and out of reset;
- renderer channel map is known;
- output buffers contain valid PCM.

Clock loss, decoder exit, renderer exit or output xrun should assert mute before restarting the affected pipeline.

## Primary references

- bytes at work, `IMX93 OSM-S` datasheet v2.1, 2024-12-06.
- NXP i.MX93 documentation / Linux `fsl_sai` driver.
- Lattice SiI9437/SiI9438 eARC Receiver data brief.
- AKM AK4458 datasheet, TDM512 and Daisy Chain sections.
- TI SN74LVC3G17 and SN74AXC4T245 datasheets.
- WONDOM/Sure Electronics KAB9 datasheet.
- `sofianchitac/VibesboxSRC`, SiI9437 eARC tap hardware evidence.
- `harletty/harletty-bridge` and `mgth/Omniphony` upstream repositories.
