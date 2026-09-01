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
slots:         16
slot width:    32 bits
BCLK:          24.576 MHz (512fs)
serial format: AK4458-compatible TDM512
```

MCLK frequency is intentionally left as a BSP/hardware configuration item until the AK4458 clock table is checked against the exact selected DIF/DFS mode. Do not hard-code an MCLK ratio from an unrelated reference design.

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

Both DACs use TDM512 and identical serial data format. Daisy-chain mode must be enabled as documented by AKM. Keep configuration access available on the carrier; do not rely on inaccessible strap-only configuration until bring-up is complete.

The AK4458 is an 8-channel differential-output DAC with 115 dB-class S/N specification and explicitly advertises TDM/daisy-chain use for multichannel applications.

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

Recommended assignment:

### KAB9-A

- FL
- FR
- C
- SL
- SR
- BL
- BR
- one reserve/test channel

### KAB9-B

- Top Front Left
- Top Front Right
- Top Rear Left
- Top Rear Right
- subwoofer on PBTL if a passive sub is used
- remaining channels reserve

For a powered subwoofer, keep the LFE path at line level and leave the corresponding KAB9 channels unused or available for future expansion.

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
4. Configure TDM512/daisy chain and channel format.
5. Generate a zero PCM stream and verify DAC lock.
6. Start renderer route.
7. Ramp software gain from silence.
8. Release KAB9 mute last.

Shutdown runs the reverse sequence; loss of decoder/render/output clock asserts amp mute first.

## 9. Hardware gates before PCB freeze

- scope SiI9437 → translator → SAI1 clock/data edges;
- prove bit-exact IEC burst capture on i.MX93;
- validate SAI3 TDM512 clocking and all 16 test-tone slots;
- verify AK4458 MCLK requirements and Linux clock tree on the selected BSP;
- characterize differential DAC-to-KAB9 gain and DC/common-mode behavior;
- measure idle noise with class-D power stages running;
- run thermal test at representative multichannel power.
