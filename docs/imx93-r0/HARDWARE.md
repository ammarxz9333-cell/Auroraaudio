# Aurora i.MX93 R0 Hardware Reference

## 1. Compute module

Selected module: **byteENGINE IMX93 OSM-S**.

Reference configuration used by R0 planning:

- NXP i.MX93;
- 2 × Cortex-A55 up to 1.7 GHz;
- Cortex-M33 available for future offload/control;
- 1 GB RAM / 16 GB eMMC base module is sufficient for the selected audio-only appliance;
- 1.8 V OSM GPIO/audio I/O domain must be respected.

Pin assignments below are taken from byteENGINE IMX93 OSM-S datasheet **v2.1 (2024-12-06)**. Verify the exact ordered module revision before PCB fabrication.

The canonical logical/TDM/DAC channel order is maintained separately in [`CHANNEL_MAP.md`](CHANNEL_MAP.md). That file overrides any informal wiring description elsewhere.

## 2. eARC ingress

### SiI9437 prototype taps

For the Lindy 38368 reference board, hardware tracing of the SiI9437 audio cluster identifies:

| SiI9437 pin | Signal | R0 use |
| --- | --- | --- |
| 10 | SCK / BCLK | SAI1 RX bit clock |
| 11 | WS / LRCLK | SAI1 RX frame sync |
| 12 | SD0 | encoded IEC 61937 data |
| 13 | SD1 | reserve for multichannel LPCM |
| 14 | SD2 | reserve for multichannel LPCM |
| 15 | SD3 | reserve for multichannel LPCM |
| 17 | S/PDIF / DSDR2 | not selected for R0 |

R0 uses BCLK, WS and SD0 only for the encoded DD+ path.

### 3.3 V → 1.8 V translation

Use one **SN74LVC3G17** powered from 1.8 V:

```text
SiI9437 BCLK ──> A1  SN74LVC3G17  Y1 ──> i.MX93 SAI1_RX_BCLK
SiI9437 WS   ──> A2                 Y2 ──> i.MX93 SAI1_RX_SYNC
SiI9437 SD0  ──> A3                 Y3 ──> i.MX93 SAI1_RX_DATA00
```

TI documents the part for 1.65–5.5 V VCC and over-voltage-tolerant inputs up to 5.5 V. Add local 100 nF decoupling and series damping footprints close to the source/translator as a prototype option; final values must be chosen from scope measurements.

### i.MX93 OSM-ext input pins

| OSM pin | MPU pin | Mux function |
| --- | --- | --- |
| AA21 | D21 | `SAI1_RX_BCLK` |
| AA20 | D20 | `SAI1_RX_SYNC` |
| V21 | H20 | `SAI1_RX_DATA00` |

SAI1 runs as RX clock slave. The SiI9437 supplies BCLK and frame sync.

## 3. Native 16-channel DAC bus

R0 uses a separate SAI instance for playback.

### i.MX93 OSM-ext output pins

| OSM pin | MPU pin | Mux function |
| --- | --- | --- |
| T3 | R21 | `SAI3_TX_BCLK` |
| M21 | V20 | `SAI3_TX_SYNC` |
| F4 | T21 | `SAI3_TX_DATA00` |
| T4 | R20 | `SAI3_MCLK` |

The v2.1 datasheet also exposes alternate pads with some of these mux functions. R0 uses the table above as the PCB target; the byteENGINE BSP pinctrl macros must be confirmed against the exact module before the carrier schematic is frozen.

Playback framing target:

```text
sample rate:   48,000 Hz
PCM format:    S32_LE
slots:         16
slot width:    32 bits
BCLK:          24.576 MHz (512fs)
serial format: DSP_B / AK4458 TDM512
```

The upstream NXP `fsl,imx-audio-card` AK4458 TDM path explicitly models the 16-channel/TDM512 case and selects an AK4458 MCLK relationship of **1024fs**, i.e. **49.152 MHz at 48 kHz**. This is the R0 software target. G6 still must prove that the exact byteENGINE/NXP i.MX93 BSP clock tree can synthesize that MCLK and the 24.576 MHz BCLK on the selected SAI3 pads without drift or xruns.

### 1.8 V → 3.3 V translation

AK4458's digital supply is a 3.3 V-class domain; the OSM is 1.8 V. Use **SN74AXC4T245** with:

```text
VCCA = 1.8 V (i.MX93 side)
VCCB = 3.3 V (DAC side)
DIR  = A → B
channels = MCLK, BCLK, LRCLK, DATA
```

TI specifies up to 380 Mbps for 1.8 V → 3.3 V translation, far above the R0 serial audio clocks.

## 4. Dual AK4458 topology

Selected DACs: **2 × AK4458VN**, each 8 channels, differential output.

Use the manufacturer's TDM512 daisy-chain topology:

```text
                      shared MCLK/BCLK/LRCK
                             │
i.MX93 DATA ─────────────> SDTI1  AK4458 #2
                              │
                              ├─ local DAC: later 8 TDM channels
                              │
                              └─ TDMO1 ─────────────> SDTI1  AK4458 #1
                                                       │
                                                       └─ local DAC: first 8 channels
```

Both DACs use the same DSP_B/TDM512 serial framing. Linux's AK4458 codec driver supports S32_LE, TDM slot configuration and enables daisy-chain mode for DSP_B/TDM playback above eight channels. This is strong software support, but G6 must still measure the actual physical split and all sixteen analog outputs.

The intended R0 split is:

```text
TDM slots 1..8   -> AK4458 #1 DAC1..DAC8
TDM slots 9..16  -> AK4458 #2 DAC1..DAC8
```

Do not wire the power amplifiers permanently until the one-slot-at-a-time G6 test confirms that exact physical mapping.

## 5. Analog interface

AK4458 full-scale output is approximately 2.8 Vpp per output leg, with approximately 5.6 Vpp differential when externally summed at unity gain. The KAB9 reference amplifier can reach full output at substantially lower input level depending on its gain switch setting.

Therefore R0 requires, per channel:

```text
AK4458 differential output
→ reconstruction / RF filtering per AKM guidance
→ DC/common-mode management
→ symmetric attenuation / buffer
→ KAB9 differential input
```

Initial target: choose analog attenuation so 0 dBFS cannot overdrive the KAB9 at the selected minimum-gain configuration. Exact resistor/op-amp values are **not frozen** until KAB9 differential input behavior is measured on the actual boards. Software headroom is additional protection, not a substitute for a sane analog gain structure.

## 6. Power amplifier reference

Two **WONDOM KAB9 / AA-KA32473** boards provide sixteen BTL amplifier channels total. Each board is specified as 8 × 50 W class-D and supports per-chip BTL/PBTL reconfiguration.

To preserve the canonical DAC order during R0 bring-up, use:

### KAB9-A

1. FL
2. FR
3. C
4. LFE power channel only if a passive-sub arrangement is selected; otherwise leave this amplifier channel unused and take LFE at line level
5. BL
6. BR
7. SL
8. SR

### KAB9-B

1. TFL
2. TFR
3. TRL
4. TRR
5. reserve
6. reserve
7. reserve
8. reserve

For a powered subwoofer, route the DAC LFE output through the line-level analog stage to the powered sub and leave KAB9-A channel 4 unused. If PBTL is later selected for a passive sub, validate that power-stage mode separately; it must not change the logical, PipeWire, TDM or DAC channel order.

## 7. Power tree

Prototype power domains should be treated separately:

```text
24 V main supply
├─ KAB9-A
├─ KAB9-B
├─ buck → 5 V / module-carrier rail
├─ low-noise 5 V analog → AK4458 AVDD/VREF domains
├─ 3.3 V digital/analog support rail
└─ 1.8 V OSM I/O translation rail
```

Do not power high-current class-D stages through the compute-module rail. Star/segmented grounding and return-current control matter more than drawing a single `GND` symbol everywhere. DAC reference/analog supplies require local low-noise decoupling following AKM guidance.

## 8. Startup and mute sequence

1. Hold both KAB9 boards muted/shutdown.
2. Start i.MX93 and configure SAI3.
3. Keep AK4458 devices in reset/power-down until clocks and control are valid.
4. Configure DSP_B/TDM512/daisy chain and channel format.
5. Generate a zero PCM stream and verify DAC lock.
6. Start the exact `aurora_tdm` renderer route.
7. Ramp software gain from silence.
8. Release KAB9 mute last only after G7 has frozen a fail-safe mute implementation.

Shutdown runs the reverse sequence; loss of decoder/render/output clock must assert amp mute before stopping clocks/software.

## 9. Hardware gates before PCB freeze

- scope SiI9437 → translator → SAI1 clock/data edges;
- prove bit-exact IEC burst capture on i.MX93;
- validate SAI3 48 kHz / S32_LE / 16ch TDM512 and the 49.152 MHz MCLK target;
- identify all sixteen dual-AK4458 analog outputs independently;
- characterize differential DAC-to-KAB9 gain and DC/common-mode behavior;
- validate any powered-sub/PBTL choice without altering the canonical channel contract;
- measure idle noise with class-D power stages running;
- run thermal test at representative multichannel power.
