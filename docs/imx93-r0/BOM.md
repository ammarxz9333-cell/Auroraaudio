# Aurora i.MX93 R0 Critical BOM

This is the selected functional BOM, not yet a PCB purchasing release. Pass the hardware gates before ordering production quantities.

| Qty | Part | Function | R0 status |
| ---: | --- | --- | --- |
| 1 | byteENGINE IMX93 OSM-S, 1 GB / 16 GB class | Linux compute + SAI input/output | selected; target hardware benchmark pending |
| 1 | Lindy 38368 for prototype | complete eARC front end containing SiI9437 | externally validated architecture; local tap pending |
| 1 | Lattice SiI9437 for future integrated carrier | eARC receiver | future integration after Lindy prototype passes |
| 1 | SN74LVC3G17 | BCLK/WS/SD0 3.3 V → 1.8 V input buffer | selected |
| 1 | SN74AXC4T245 | SAI3 MCLK/BCLK/LRCLK/DATA 1.8 V → 3.3 V | selected |
| 2 | AK4458VN | 16-channel TDM512 DAC stage | selected; daisy-chain hardware gate pending |
| 2 | WONDOM KAB9 / AA-KA32473 | power amplification | selected reference amplifier |
| 1 | 24 V power supply sized for actual speaker/power target | main amp rail | size after thermal/power test |
| 1 | 5 V compute supply / buck stage | i.MX93 carrier rail | carrier-dependent |
| — | low-noise regulators/filters | AK4458 analog/reference rails | schematic stage |
| 12 | analog LPF/attenuation channels | DAC → KAB9 conditioning | values pending measurement |

## Channel budget

```text
DAC channels available: 16
7.1.4 required:          12
reserve:                  4

KAB9 BTL channels raw:   16
required full-range:     11
LFE/sub:                   1
reserve depends on whether the sub uses PBTL or an external powered amp
```

The four spare DAC channels are deliberately retained for measurement, future front-wide channels, additional sub outputs, or layout experiments.
