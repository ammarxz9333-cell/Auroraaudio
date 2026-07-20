# VIM3L eARC reproduction and alternative-board assessment

## Executive conclusion

Khadas VIM3L remains the only currently evidenced low-cost Linux board with a publicly documented successful 7.1 LPCM capture path from a television over HDMI eARC.

The decisive evidence is not the S905D3 feature list alone. The decisive evidence is the complete combination of:

- Amlogic S905D3 eARC receiver hardware;
- board-level routing of the eARC differential pair to the HDMI connector;
- a vendor-derived Linux 5.4 driver stack;
- a working device-tree configuration;
- a published ALSA control sequence advertising multichannel PCM capability;
- independent confirmation that audio was recorded from a television through eARC.

No other inexpensive board discovered in this research has equivalent end-to-end evidence.

## Reproduced public evidence

A Khadas community developer ported the Amlogic 5.4 stack to VIM3L and booted the board using the `sm1_s905d3_ac200` device tree as a starting point. USB, SD, eMMC, Ethernet and the eARC receiver were brought up, while display output was not required for the audio-capture use case.

The initial eARC state exposed only failsafe capabilities: stereo PCM and compressed 5.1. The developer later determined that format negotiation is performed through the eARC communication channel rather than CEC.

The following ALSA control payload advertised 7.1 PCM support and all reported sample/bit rates:

```bash
amixer cset numid=6,iface=MIXER,name='eARC_RX CDS' \
  0x01,0x01,0x08,0x23,0x0f,0x7f,0x07,0x83,0x0f,0x40,0x00,0x00
```

After this control was written, the developer reported working 7.1 PCM capture. A second forum participant independently reported successfully recording audio from a television through eARC.

## Kernel and source status

Khadas currently documents Linux 5.15 sources for VIM3/VIM3L, with common drivers and device trees stored separately. The older Linux 4.9 tree is explicitly not maintained.

The successful community implementation used an Amlogic-derived Linux 5.4 tree rather than the original 4.9 implementation, because the 4.9 eARC code was incomplete for the required capture path.

The practical Aurora baseline must therefore preserve the exact known-good community branch or reconstruct its changes against a maintained Khadas/Amlogic tree before treating VIM3L as production-capable.

## Board-level constraint

The S905D3 SoC exposes dedicated `eARC_P` and `eARC_N` input pins and contains an eARC receive path. This does not mean every S905D3 product supports eARC capture.

A candidate board is acceptable only when all of the following are proven:

1. `eARC_P` and `eARC_N` are routed to the correct HDMI connector pins;
2. the board includes the required analogue protection and passive network;
3. the device tree enables an eARC RX node;
4. the sound-card topology exposes an ALSA capture interface;
5. the kernel includes the Amlogic eARC RX driver and related clock/reset support;
6. userspace can set the eARC CDS capability block;
7. multichannel LPCM capture is demonstrated, not inferred from the SoC datasheet.

Most generic S905D3 TV-box boards list HDMI only as a display output and do not publish proof that the eARC differential pair is connected. They must not be purchased as substitutes without schematics or continuity evidence.

## Alternative S905D3 boards

### Libre Computer AML-S905D3-CC Solitude

This board uses S905D3, has public U-Boot support and published schematics. However, no evidence was found that its HDMI connector routes and supports the eARC receive pair, and no successful Linux eARC capture report was found.

Status: hardware audit candidate only, not a validated substitute.

### CEK8902-S905D3

This development board exposes an HDMI 2.1 display-output connector and a 40-pin header. Public material does not claim eARC capture, does not document the relevant differential routing, and provides no Linux ALSA capture evidence.

Status: unverified; do not buy for eARC without schematic confirmation.

### Generic Chinese S905D3 boards

Several OEM boards advertise Android 9, HDMI output, USB, Ethernet and optional development materials. Their public descriptions do not advertise eARC receive operation. The HDMI connector may be wired solely for transmission.

Status: unsuitable until the manufacturer confirms schematic routing of `eARC_P/N`, supplies Linux source, and demonstrates multichannel capture.

## Device-tree evidence

Amlogic-derived device trees contain an `amlogic, sm1-snd-earc` node with register regions for RX CMDC, RX DMAC and RX TOP, dedicated clocks, and an `earc_rx` interrupt. On unrelated boards the node may exist but remain disabled.

Khadas community inspection reported VIM3L device-tree configuration with `&earc { status = "okay"; };`, while VIM4 configurations have appeared with the eARC node disabled. A device-tree node alone remains insufficient without physical routing and a functional driver stack.

## Aurora prototype architecture

```text
Television eARC output
        |
        v
Khadas VIM3L eARC RX
        |
        v
ALSA multichannel capture
        |
        v
Aurora input/capability adapter
        |
        +--> Aurora DSP/rendering
        |
        +--> network transport
        |
        +--> USB or multichannel DAC output
```

Aurora must treat this input as negotiated LPCM. No claim is made that the path decodes Dolby TrueHD, DTS-HD MA or object metadata. The television/source must deliver compatible multichannel PCM.

## Required reproducibility package before hardware purchase approval

The project should collect or reconstruct:

- exact VIM3L eARC kernel repository and commit;
- exact device-tree blob/source;
- kernel configuration;
- bootloader configuration;
- root filesystem and userspace versions;
- ALSA card/device listing;
- mixer control enumeration;
- startup service applying the CDS payload;
- known-good `arecord` command;
- channel-identification WAV fixture;
- recovery sequence for TV standby, HDMI disconnect and eARC renegotiation.

## Mandatory test matrix

1. Stereo PCM 44.1 and 48 kHz.
2. 5.1 PCM 48 kHz.
3. 7.1 PCM 48 kHz.
4. 24-hour continuous 7.1 capture.
5. Repeated television standby/wake cycles.
6. HDMI cable disconnect/reconnect.
7. Television eARC disable/enable.
8. Source switching.
9. Sample-rate changes.
10. Channel-order impulse test after every renegotiation.
11. XRUN and discontinuity counters.
12. Capture-to-output latency measurement.
13. Thermal and CPU-load test with Aurora DSP active.
14. Network congestion test while capture continues.
15. Cold boot without manual television setting changes.

## Decision

VIM3L remains the first hardware candidate for Aurora eARC capture.

No generic S905D3 board is an approved substitute merely because it uses the same SoC. A substitute requires board-level routing proof and an end-to-end Linux capture demonstration.

The next research checkpoint is source preservation and build reproduction, not further speculative SoC selection.
