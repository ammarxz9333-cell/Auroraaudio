# EP91A7E and LT6911 board research

Status: active hardware research
Date: 2026-07-20

## Purpose

Record the current evidence for practical HDMI/eARC to multichannel digital-audio hardware candidates, avoid repeated searches, and separate confirmed capability from unresolved implementation risk.

## Current conclusion

No low-cost, openly documented, retail-ready board was found that exposes HDMI/eARC-derived 8-channel LPCM as USB Audio Class or as a Linux-ready capture device.

The strongest silicon candidate for an Aurora-owned bridge is currently Explore Microelectronics EP91A7E because it combines:

- HDMI 1.4/2.0/2.1 repeater capability up to 4K60;
- ARC/eARC receive;
- integrated HDCP 1.4/2.3 engine and keys;
- EDID memory and embedded eFlash MCU;
- 8-channel IIS output with four data lines, word clock, bit clock and MCLK;
- SPDIF output;
- extraction of LPCM, DSD and HBR audio.

However, the public documentation is marked confidential/NDA-required, no public reference design or programming package was found, and no verified evaluation board available to an individual buyer was found.

## EP91A7E evidence

### Confirmed

Manufacturer and distributor material identifies EP91A7E as a one-input, one-output HDMI repeater with eARC/ARC receiving and multichannel digital audio output.

The available datasheet copy shows the following output pins:

- IIS_SD0
- IIS_SD1
- IIS_SD2
- IIS_SD3
- IIS_WS
- IIS_SCK
- MCLK
- SPDIF

This is sufficient at the electrical-interface level for eight LPCM channels using four stereo I2S data lanes.

The chip also contains an embedded MCU, eFlash, EDID memory and integrated HDCP keys. This reduces the need for a separate licensed HDCP-key storage device, but does not remove the need to obtain the vendor configuration, firmware, reference schematic and compliance terms.

JLCPCB lists EP91A7E as an extended assembly part and exposes an EasyEDA symbol/footprint. The listing does not constitute availability of firmware, HDCP authorization, reference design, or a working board. It also states that such parts may be stored for assembly rather than shipped separately.

### Not yet confirmed

- public evaluation-board ordering path;
- source code or register programming guide;
- firmware image and flashing process;
- reference schematic, power tree and PCB layout constraints;
- exact eARC negotiation behavior with common televisions;
- whether uncompressed 7.1 LPCM is emitted in a stable, documented I2S framing mode;
- HBR handling and whether compressed Atmos/TrueHD is merely forwarded or can be decoded;
- individual-buyer access to NDA documents and engineering support;
- legal/commercial conditions for HDCP-enabled production.

### Aurora suitability

EP91A7E should be treated as a high-value custom-hardware candidate, not as a ready prototype purchase.

Potential architecture:

TV eARC -> EP91A7E -> 4 x I2S data + clocks -> FPGA/MCU/SoC capture bridge -> Aurora DSP

A direct Raspberry Pi GPIO connection is not assumed. Clock-domain ownership, I2S framing, electrical levels, DMA capture and Linux ASoC integration require an explicit bridge design and validation.

## LT6911 family evidence

### LT6911GX

Confirmed capability:

- HDMI 2.1 receiver;
- 8-channel LPCM I2S output;
- SPDIF output;
- sample rates up to 192 kHz;
- integrated processor with SPI-flash firmware;
- I2C control;
- primary video output through MIPI/LVDS variants.

### Risks

- no eARC receive capability was confirmed for LT6911GX;
- the part is primarily a display/VR bridge, not an audio or soundbar-oriented eARC device;
- firmware is required for automatic operation;
- no verified open firmware or complete public programming package was found;
- BGA packaging and high-speed video routing make a custom board substantially harder than EP91A7E QFN implementation;
- available Linux drivers focus on video bridge operation, not ALSA multichannel audio capture.

### Open-hardware assistance

Antmicro publishes a KiCad symbol and footprint for LT6911UXC, including I2S pins. This is useful only as component-library assistance; it is not a working audio bridge design and the UXC pinout shown exposes only one audio data output, so it must not be assumed equivalent to GX eight-channel output.

### Aurora suitability

LT6911GX remains relevant for source-HDMI extraction or display-linked prototypes, but it is currently lower priority than EP91A7E for the television-eARC requirement.

## Other confirmed reference platforms

### TI DS90UB949-Q1EVM

The TI evaluation module accepts HDMI and exposes support for up to eight I2S channels at up to 192 kHz, but its purpose is HDMI-to-FPD-Link III serialization. It is an engineering reference for HDMI audio extraction, not an eARC capture solution and not a clean Aurora product architecture.

### Analog Devices EVAL-MELODY-8

This platform combines ADV7625 with SHARC processing and supports HDMI audio decoding and multichannel analog output. Ordering requires HDCP licensing. It validates the general architecture but is expensive, closed and unsuitable as Aurora's low-cost foundation.

## Previously researched solutions consolidated

### ezsound 6x8

Best current Raspberry Pi multichannel codec platform for hardware validation.

- PCM3168A;
- six analog inputs and eight analog outputs;
- Raspberry Pi 5;
- ASoC/device-tree work;
- onboard clocking and isolation;
- demonstrated simultaneous multichannel operation and CamillaDSP use.

It does not provide HDMI, ARC/eARC, Dolby/DTS decoding or wireless speakers.

### SupTronics X6000/X7000

Rejected as production foundation.

Architecture uses EP91A6S and multiple ES9023 DACs, but independent reports document channel-order corruption after underruns/buffer faults. It may remain useful for destructive testing or comparison only.

### Evolve II-4K / CYP AU-11SA-4K22

Closed commercial extractor. Reports indicate better stability after EDID tuning than X6000/X7000. Useful only as a benchmark/prototype analog source; it does not expose a Linux-native digital capture interface.

### miniDSP Flex HT/HTx/HTn

Commercial proof that televisions can deliver up to eight LPCM channels over eARC into DSP hardware. Flex HTn additionally exports channels over Dante/AES67. It proves feasibility but does not provide a low-cost open hardware foundation or USB capture output.

### ARVUS AES-16H

Professional HDMI/AES/Dante/AES67 bridge supporting high channel counts. Strong architectural proof, but cost and closed design exclude it as an Aurora consumer foundation.

### ADV7611 / ADV7625 / AD1939 / CS42448 / PCM3168A / ADAU1467

Remain useful as component and reference-design evidence. ADV7625 is well documented but older and entangled with HDCP access. PCM3168A is currently the most attractive codec for a multichannel prototype. None alone solves eARC-to-Linux capture.

## Architecture ranking

1. EP91A7E custom eARC-to-I2S bridge, conditional on vendor access and compliance.
2. Commercial eARC-to-Dante/AES67 bridge as an expensive validation/reference path.
3. Source-device HDMI extraction with LT6911GX or similar when television eARC is not required.
4. Closed analog extractor feeding multichannel ADC, prototype only.
5. SupTronics X6000/X7000, rejected for reliability.

## Immediate research queue

1. Request from Explore Microelectronics or SEMICONN:
   - EP91A7E evaluation board;
   - reference schematic and BOM;
   - programming guide and firmware;
   - sample availability for an individual/open-source research project;
   - HDCP and production-license requirements;
   - confirmation of 8-channel LPCM output framing from eARC.
2. Search product teardown databases and FCC/CE internal photos for devices using EP91A7E or EP91A7P.
3. Identify soundbar/eARC extractor PCBs exposing test pads for IIS_SD0..3, WS, SCK and MCLK.
4. Evaluate an FPGA or RP1/CM4/CM5-compatible TDM/I2S capture bridge; do not assume Raspberry Pi GPIO can safely capture four synchronous I2S data lines.
5. Create a hardware validation plan covering EDID, eARC discovery, sample-rate changes, mute/relock behavior, channel mapping, underruns and power-cycle recovery.
6. Keep Dolby/TrueHD decoding out of scope unless a licensed decoder path is explicitly selected. LPCM transport and compressed bitstream forwarding are separate capabilities.

## Decision gate

Do not design an EP91A7E PCB until all of the following are obtained:

- usable reference schematic;
- firmware/programming access;
- confirmed sample or board procurement;
- explicit HDCP/compliance path;
- confirmed eARC 8-channel LPCM output format;
- capture-side clocking and Linux integration design.

Until then, use ezsound 6x8 for DSP/output validation and treat commercial eARC/Dante products as behavioral references only.
