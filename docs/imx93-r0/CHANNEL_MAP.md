# Aurora R0 Channel Contract

This file is the single software-to-wire channel-order contract for R0. Do not reorder one layer without updating and re-validating every layer below it.

## Logical 7.1.4 order

| Index | Aurora/Omniphony | PipeWire SPA position | TDM slot | DAC output |
|---:|---|---|---:|---|
| 0 | FL | FL | 1 | AK4458 #1 DAC1 |
| 1 | FR | FR | 2 | AK4458 #1 DAC2 |
| 2 | C | FC | 3 | AK4458 #1 DAC3 |
| 3 | LFE | LFE | 4 | AK4458 #1 DAC4 |
| 4 | BL | RL | 5 | AK4458 #1 DAC5 |
| 5 | BR | RR | 6 | AK4458 #1 DAC6 |
| 6 | SL | SL | 7 | AK4458 #1 DAC7 |
| 7 | SR | SR | 8 | AK4458 #1 DAC8 |
| 8 | TFL | TFL | 9 | AK4458 #2 DAC1 |
| 9 | TFR | TFR | 10 | AK4458 #2 DAC2 |
| 10 | TRL | TRL | 11 | AK4458 #2 DAC3 |
| 11 | TRR | TRR | 12 | AK4458 #2 DAC4 |
| 12 | reserve | AUX0 | 13 | AK4458 #2 DAC5 |
| 13 | reserve | AUX1 | 14 | AK4458 #2 DAC6 |
| 14 | reserve | AUX2 | 15 | AK4458 #2 DAC7 |
| 15 | reserve | AUX3 | 16 | AK4458 #2 DAC8 |

Notes:

- Omniphony's semantic label resolver accepts `TRL` as the Top-Back-Left label and `TRR` as Top-Back-Right. We intentionally use these spellings because PipeWire SPA uses `TRL`/`TRR`; its Bluetooth mapping explicitly maps external TBL/TBR terminology onto SPA TRL/TRR.
- Omniphony converts `C -> FC`, `BL -> RL`, and `BR -> RR` before publishing PipeWire channel positions. The table above records the resulting PipeWire names.
- The physical AK4458 daisy-chain direction is: i.MX93 SDATA -> AK4458 #2 -> TDMO1 -> AK4458 #1. The intended split is slots 1..8 on #1 and slots 9..16 on #2. G6 must still identify every analog output physically before amplifier wiring is frozen.
- PipeWire exposes a 16-channel hardware sink because the NXP `fsl,imx-audio-card` AK4458 TDM path constrains TDM channel counts to 1..8 or 16; 12 is not a supported TDM hardware count. Omniphony's 12-channel stream is position-mapped into the 16-channel sink, with AUX0..AUX3 unused.

## Amplifier reference wiring

For the simplest R0 wiring, preserve DAC order into the amplifier stage:

### KAB9-A

1. FL
2. FR
3. C
4. LFE analog feed if a passive-sub amplifier channel is used; otherwise leave this power channel unused and route DAC LFE to a line-level sub output
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

Any PBTL subwoofer configuration changes the power-stage wiring only. It must not change the logical/TDM/DAC order above.

## Validation

Before connecting normal speakers, play a one-slot-at-a-time 16-channel test file at a low level and record the physical connector reached by each slot. G6 passes only when that measured table matches this contract or this file is corrected to the measured reality.
