# OEM eARC modules and EP92H1E findings

## Executive summary

A commercially presented module now exists that closely matches Aurora's missing bridge:

TV eARC -> module -> 4-lane I2S (up to 8-channel PCM, 24-bit/192 kHz)

Circal Engineering advertises an eARC/ARC Audio Receiver Module with:
- up to 8-channel PCM at 24-bit/192 kHz
- four I2S output lanes in eARC mode
- SPDIF output for legacy ARC
- automatic ARC/eARC negotiation
- SPI control and status
- host firmware update support
- single 5 V supply
- standby and wake support

This is not confirmed as an off-the-shelf retail product. Circal markets it as an integration module and asks customers to start a project, so price, MOQ, access for individuals, API availability, connector pinout, and licensing remain unknown.

## Related Circal platforms

Circal also advertises:
- a stereo development system containing the eARC receiver module
- a surround-sound development system
- a Dolby Atmos / DTS:X decompression module
- USB/I2S and network streamer modules

These demonstrate that the eARC module is part of a real product-development platform rather than only a concept page. However, no public retail pricing or downloadable integration package was found.

## Explore Semiconductor EP92H1 family

Explore Semiconductor publishes the EP92H1 family as HDMI 2.1-compliant ARC/eARC audio interface converters.

Relevant receive capabilities:
- ARC/eARC RX to I2S, SPDIF, DSD, or HBR
- up to 8-channel LPCM
- compressed IEC-61937/HBR transport
- channel-status and audio-infoframe extraction
- bidirectional ARC/eARC support

Important package variants:
- EP92H1: 56-pin QFN
- EP92H1E: 64-pin LQFP

The LQFP EP92H1E is especially interesting for Aurora because it is easier to prototype and assemble than fine-pitch QFN alternatives such as IT6620BFN. Public information still does not include the full programming guide, reference schematic, firmware/API package, or retail distributor availability.

## NXP i.MX Audio Board

The NXP MCIMX8M-AUD remains a strong reference architecture:
- HDMI input
- HDMI eARC
- SPDIF I/O
- 24-channel DAC line output
- Linux support

NXP currently lists the full kit at USD 1,840 and notes that purchases are subject to approval. This makes it unsuitable as Aurora's low-cost prototype but highly useful as a validated architecture and software reference.

A 2026 upstream device-tree patch for the i.MX8M Plus Audio Board shows NXP is still actively mainlining support for this platform. NXP support also states that eARC is controlled by the Cortex-A side and does not require an FPGA to transfer audio internally.

## Commercial HDMI-only extractors

Several retail products pass full eARC audio to another HDMI audio endpoint, including 7.1 PCM and lossless compressed formats, but do not expose raw multichannel PCM over USB, I2S, or analog outputs. These are useful as protocol references or donor-board candidates, not direct Aurora bridges.

Examples include:
- ThenAudio SHARC-V2
- Blustream SM11EARC-8K
- J-Tech Digital JTECH-8KAE
- DAIAD DHD-eARC-AD8K

Their optical and analog outputs remain limited to stereo PCM or compressed 5.1, so they do not solve Aurora's Linux multichannel capture requirement.

## Current ranking

1. Circal eARC/ARC receiver module, if sold in single quantities with documentation
2. Khadas VIM3L native eARC capture, if source can be recovered or ported
3. EP92H1E custom Aurora board
4. IT6620BFN plus XMOS USB Audio bridge
5. NXP MCIMX8M-AUD as reference only
6. EP91A7E / IT66320 / IT66326 for architectures requiring HDMI switching or passthrough

## Required diligence before procurement

For Circal:
- unit price and MOQ
- whether individuals/open-source projects are accepted
- module dimensions and connector pinout
- electrical I2S format and clock-master/slave options
- SPI API and firmware terms
- CDS/EDID configuration support
- licensing restrictions
- sample availability

For EP92H1E:
- distributor and minimum order
- public or obtainable evaluation board
- full datasheet and register guide
- reference schematic
- required MCU firmware
- whether unencrypted LPCM operation is possible without HDCP licensing
- exact I2S lane mapping and clock behavior

## Decision

The Circal module is the first publicly described integration module that directly matches Aurora's missing eARC-to-I2S bridge. It deserves immediate vendor-contact priority. EP92H1E is the most promising custom-board fallback because its LQFP package lowers prototype difficulty.
