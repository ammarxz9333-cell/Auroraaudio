# Aurora realtime-MCU pinmux proof

Status: **datasheet-verified routing contract; not physical measurement**.

## Single source of truth

All concrete target/package/pin/ball/alternate-function values live only in:

```text
config/aurora-hardware-target.env
```

This document deliberately does **not** duplicate the active part number or pin table. Runtime, CI, HAL generation and future PCB tooling must consume the symbolic `AURORA_REALTIME_MCU_*`, `AURORA_PIN_*`, `AURORA_BALL_*` and `AURORA_AF_*` names from that manifest.

A future MCU/package change must therefore begin by changing the manifest, not by editing Aurora protocol/DSP/runtime code.

## Proof method

The selected target was checked against the manufacturer's current datasheet and reference documentation for these simultaneously required roles:

1. USB 2.0 High-Speed Host through external ULPI PHY;
2. direct ULPI digital pins rather than package-specific analog-switch `_C` routing;
3. eARC/IEC61937 capture through one SAI block as external-clock slave RX;
4. 7.1.4 PCM output through a separate SAI block in TDM mode;
5. optional TDM master-clock output;
6. dedicated GPIO reserve for USB PHY reset;
7. dedicated fail-closed amplifier mute GPIO;
8. no MCU pin reused by two live roles.

CI enforces the no-conflict rule by rejecting duplicate `AURORA_PIN_*` assignments and validates the declared alternate-function groups.

## Why direct ULPI pins are mandatory

Some packages expose ULPI DIR/NXT only through `Pxy_C` analog-switch paths. The datasheet documents package-specific electrical limits for those `_C` pins. ST technical guidance in January 2026 explicitly recommended using the package with direct `PC2`/`PC3` exposure when USB High-Speed ULPI is required.

Aurora therefore treats `AURORA_REALTIME_MCU_ULPI_DIRECT_PINS=1` as a mandatory production-target capability. A cheaper package that requires the `_C` analog-switch path does not satisfy the current reliability target.

## Audio routing contract

### eARC capture

The symbolic `AURORA_EARC_SAI_*` and `AURORA_PIN_EARC_*` fields select one SAI block for:

```text
external BCLK/SCK -> SAI slave clock
external WS/FS    -> SAI frame sync
receiver SD0      -> SAI serial data input
```

No eARC-capture MCLK is required. The physical receiver remains the clock master.

### 7.1.4 TDM output

The symbolic `AURORA_TDM_SAI_*` and `AURORA_PIN_TDM_*` fields select a separate SAI block for:

```text
12 active channels
48 kHz
32-bit slots
TDM frame
DMA-backed transmit
```

The STM32H723-family SAI supports TDM operation and a slot register with up to 16 slots, leaving headroom for the 12-channel Aurora v1 layout. The final DAC topology still must be selected and physically validated before the TDM electrical format is called accepted hardware.

## Primary references

- ST DS13313 Rev 5 — STM32H723xE/G datasheet, especially package pin/ball descriptions and alternate-function Table 8:
  - https://www.st.com/resource/en/datasheet/stm32h723zg.pdf
- ST product page for package/order-code and lifecycle state:
  - https://www.st.com/en/microcontrollers-microprocessors/stm32h723zg
- ST RM0468 — SAI/USB implementation reference manual:
  - https://www.st.com/resource/en/reference_manual/dm00603761.pdf
- ST technical discussion on H723 ULPI through `_C` pins and recommendation for direct PC2/PC3 package routing, January 2026:
  - https://community.st.com/t5/stm32-mcus-products/stm32h723-ulpi-and-pc2-c-pc3-c/td-p/762175

## Acceptance boundary

`AURORA_REALTIME_MCU_PINMUX_STATUS=datasheet_verified` means only:

- the required functions exist on the selected package;
- the manifest assignments do not conflict;
- alternate-function selections were checked against manufacturer documentation.

It does **not** mean:

- PCB routing has been reviewed;
- signal integrity has been measured;
- ULPI enumerates the Galaxy S6;
- SAI DMA works on physical hardware;
- TDM timing is accepted by the final DAC;
- amplifier mute has been electrically verified.

Those remain under `AURORA_REALTIME_MCU_HAL_STATUS` and `AURORA_REALTIME_MCU_PHYSICAL_STATUS` and must stay fail-closed until real hardware evidence exists.
