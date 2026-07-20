# Alternative eARC Bridge Solutions

## Purpose

This note records additional solutions beyond EP91A7E and Lontium LT6911 for converting TV eARC or HDMI multichannel audio into a digital interface suitable for Aurora.

## Highest-priority new candidate: ITE IT6620BFN

The ITE IT6620BFN is a dedicated HDMI 2.1 eARC/HDMI 1.4 ARC receiver intended for audio endpoints. Unlike HDMI repeater devices, it does not require a video path when eARC is active.

Key documented capabilities:

- HDMI 2.1 eARC receiver and HDMI 1.4 ARC compatibility.
- Up to 98.304 Mbit/s eARC DMAC bandwidth.
- Up to 8-channel 192 kHz LPCM from a television.
- Up to 16-channel LPCM at reduced maximum sample rate.
- Eight I2S outputs.
- TDM output supporting 4, 8, or 16 channels on one serial data interface.
- SPDIF output, including compressed and HBR transport modes.
- Embedded eARC capability-data registers.
- Embedded CEC PHY.
- 40-pin 5 mm x 5 mm QFN package.

### Why this is stronger than EP91A7E for Aurora

IT6620BFN is an audio-only eARC receiver. Therefore it avoids:

- HDMI 2.0 video routing;
- HDCP video handling;
- high-speed TMDS video PCB routing;
- unnecessary repeater logic;
- an 88-pin package.

A likely Aurora architecture is:

TV eARC -> IT6620BFN -> 8-channel TDM/I2S -> FPGA, DSP, USB bridge, or Linux-capable audio interface.

### Remaining blockers

- Full datasheet and programming guide are not publicly available.
- No public evaluation board was found.
- No public Linux driver was found.
- Firmware or MCU initialization remains necessary for eARC capabilities, latency reporting, mute behavior, and ARC compatibility.
- Availability through normal authorized distributors remains unconfirmed.

The device should not yet be used for PCB design until ITE or an authorized design house provides the reference schematic, register guide, initialization example, and sample access.

## ITE IT66320

The IT66320 is a 1-input/1-output HDMI 2.0 repeater with an HDMI 2.1 eARC receiver and embedded MCU/flash.

Relevant capabilities:

- eARC reception up to 8-channel 192 kHz LPCM;
- four I2S interfaces for eight audio channels;
- TDM, SPDIF, and DSD interfaces;
- HDMI audio extraction and re-embedding;
- embedded HDCP engines and keys;
- embedded MCU and flash.

### Evaluation

This is useful when Aurora must also pass video through or switch HDMI sources. It is less attractive than IT6620BFN for an audio-only bridge because it restores the difficult high-speed HDMI video and HDCP design burden.

Status: secondary candidate, not first prototype choice.

## ITE IT66326

IT66326 is a newer HDMI 2.1 switch/retimer with eARC, 8-channel 192 kHz LPCM, and I2S/TDM extraction. It supports substantially higher video bandwidth than IT66320.

### Evaluation

This is relevant only for a future premium Aurora HDMI hub requiring HDMI 2.1 video switching. It is excessive for the first eARC audio bridge and would increase PCB, firmware, compliance, and cost risk.

Status: future premium architecture reference.

## ITE IT6622

IT6622 combines an HDMI 1.4 transmitter, eARC receiver, embedded MCU, and multichannel serial audio extraction.

### Evaluation

Potentially useful if a design needs to generate an HDMI output while receiving eARC. For a minimal TV-to-Aurora audio bridge it remains more complex than IT6620BFN.

Status: niche alternative.

## Professional digital bridges

### miniDSP Flex HTn

Proves a complete commercial path:

TV eARC LPCM -> DSP -> Dante/AES67.

It accepts up to 8-channel LPCM over eARC and can expose eight network-audio channels. It is closed and relatively expensive, but is an excellent interoperability and latency reference.

### ARVUS HDMI/AES network bridges

Professional products demonstrate HDMI PCM to Dante/AES67 conversion at higher channel counts. They validate the network-audio architecture but are too expensive and closed for Aurora's target hardware.

### Extron MediaPort 300

The MediaPort 300 bridges HDMI and audio into USB for conferencing and supports AES67. Public material does not establish that it exposes arbitrary 7.1 HDMI LPCM as an eight-channel UAC capture device, so it must not be treated as a solved Aurora path.

Status: investigate only if teardown or precise USB descriptor evidence becomes available.

## ADAT/AES3 intermediate path

A possible modular architecture is:

TV eARC -> eARC/I2S bridge -> FPGA or format converter -> ADAT/AES3 -> class-compliant USB audio interface -> Linux.

Advantages:

- mature professional audio interfaces;
- deterministic channel mapping;
- galvanic isolation possible with ADAT;
- standard Linux support at the USB interface.

Disadvantages:

- additional hardware stages;
- clock-domain and sample-rate-conversion complexity;
- ADAT channel count falls at 96/192 kHz unless SMUX is used;
- no currently found low-cost direct eARC-to-ADAT product.

Status: viable fallback architecture, not a ready product.

## FPGA bridge path

The most credible custom bridge is:

IT6620BFN TDM/I2S -> small FPGA -> USB Audio Class 2 or PCIe/I2S endpoint.

The FPGA would provide:

- capture of one TDM stream or four/eight I2S data lines;
- deterministic channel framing;
- elastic buffering and clock-domain crossing;
- channel-order monitoring;
- optional packetization to USB, ADAT, AES3, or Ethernet;
- explicit underrun and discontinuity handling.

This avoids the channel-swap failure observed in consumer HDMI extractor boards, but it requires a new FPGA and USB/audio firmware workstream.

## Current ranking

1. ITE IT6620BFN plus TDM-to-USB/Linux bridge.
2. EP91A7E plus I2S/TDM bridge.
3. miniDSP Flex HTn as commercial validation/reference hardware.
4. IT66320 if HDMI video pass-through is required.
5. ADAT/AES3 intermediate bridge.
6. LT6911 family for HDMI source extraction rather than TV eARC.
7. Analog extraction followed by ADC for temporary prototyping only.

## Required next evidence

Before committing to IT6620BFN:

1. Obtain the complete datasheet.
2. Obtain the programming guide.
3. Obtain a reference schematic and PCB layout guidance.
4. Confirm sample or evaluation-board availability for an individual developer.
5. Confirm exact TDM slot format, clock-master behavior, sample-rate transitions, and channel-status reporting.
6. Confirm how uncompressed LPCM and HBR compressed streams are distinguished.
7. Confirm whether an external MCU is mandatory and whether initialization source code is supplied.
8. Determine the cleanest bridge target: XMOS UAC2, FPGA UAC2, PCIe, or direct SoC TDM capture.

## Research conclusion

The strongest new direction is not another complete HDMI receiver. It is a dedicated audio-only eARC receiver. IT6620BFN materially reduces hardware complexity and should replace EP91A7E as the first custom-board candidate, subject to documentation and sample access.