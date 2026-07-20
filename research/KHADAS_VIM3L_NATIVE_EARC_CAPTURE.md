# Khadas VIM3L Native eARC Capture Assessment

## Executive conclusion

Khadas VIM3L is the first publicly documented low-cost board found in Aurora research with evidence of successful 7.1 LPCM capture from a television over eARC directly into Linux.

This is materially stronger evidence than chip briefs or closed commercial products because a community developer reported working 7.1 PCM capture, shared the required eARC capability data command, and another user independently confirmed successful audio recording from a TV.

## Architecture

TV eARC
→ HDMI connector on VIM3L
→ Amlogic S905D3 integrated eARC receiver
→ Amlogic audio DMA / ALSA capture
→ Aurora DSP
→ multichannel output or network transport

No separate EP91A7E, IT6620, LT6911, HDMI extractor, FPGA, or USB capture bridge is required for the input path if this implementation can be reproduced reliably.

## Hardware

Board: Khadas VIM3L
SoC: Amlogic S905D3
Relevant characteristics:

- eARC hardware integrated in SoC;
- eARC line connected on the VIM3L PCB according to board/community documentation;
- Linux-capable ARM platform;
- Gigabit Ethernet;
- USB and expansion connectivity;
- available as a retail SBC rather than an NDA-only component.

## Public implementation evidence

A community developer ported the newer Amlogic 5.4 audio/eARC stack to VIM3L and reported:

- eARC link operation;
- initial stereo PCM and compressed 5.1 capture;
- successful 7.1 LPCM capture after advertising the correct eARC capability data structure;
- Linux CEC support used for system integration;
- a published kernel branch for the experiment.

The reported mixer command for advertising 7.1 PCM capability was:

```sh
amixer cset numid=6,iface=MIXER,name='eARC_RX CDS' \
  0x01,0x01,0x08,0x23,0x0f,0x7f,0x07,0x83,0x0f,0x40,0x00,0x00
```

Another community participant then reported successful recording of audio via eARC from a television.

## Evidence status

Status: PARTIALLY VERIFIED / HIGH-PRIORITY REPRODUCTION CANDIDATE

Verified from public evidence:

- the board and SoC have the relevant eARC hardware path;
- a Linux eARC driver exists in Amlogic vendor kernels;
- 7.1 PCM capture was reported working;
- independent confirmation of TV audio recording exists;
- the necessary eARC capability advertisement was published.

Not yet verified by Aurora:

- exact kernel commit and build reproducibility;
- stable operation over long-duration testing;
- channel-order stability after underrun, link reset, format switch, TV standby, and hotplug;
- latency and clock drift;
- compatibility across TV brands;
- 24-bit and 192 kHz modes;
- behavior with Dolby MAT output from a TV;
- whether current VIM3L stock remains readily available;
- integration with Aurora's current Rust realtime pipeline.

## Important limitation

This path receives LPCM supplied by the television. It does not itself license or decode Dolby TrueHD, DTS-HD, Atmos, or DTS:X bitstreams.

The TV or source must output multichannel LPCM. Atmos metadata may be lost unless the source renders it to channels before transmission or a licensed decoder exists upstream.

## Comparison with current candidates

### Versus IT6620BFN + XMOS

Advantages:

- already integrated hardware;
- public Linux implementation evidence;
- no custom PCB required for the first prototype;
- no immediate NDA dependency;
- no external USB bridge required;
- lower engineering risk for validation.

Disadvantages:

- vendor kernel dependence;
- older board and SoC;
- incomplete upstream support;
- long-term supply uncertainty;
- community implementation may require maintenance and hardening.

### Versus EP91A7E or LT6911

The VIM3L path is much stronger for an immediate Aurora prototype because it has demonstrated Linux capture. EP91A7E and LT6911 remain custom-hardware candidates but require documentation, firmware, PCB design, and a capture bridge.

### Versus miniDSP Flex HTn

VIM3L is open and programmable and can feed Aurora directly. Flex HTn is commercially validated but closed and substantially more expensive.

## Recommended Aurora action

Promote VIM3L to the top immediate hardware-validation candidate.

Do not declare it production architecture yet. First reproduce the public implementation and subject it to the same evidence discipline as Aurora simulation work.

## Required reproduction campaign

1. Acquire one VIM3L board.
2. Archive board revision, schematic, bootloader, DTB, kernel source, configuration, and userspace image.
3. Build and boot the known working Amlogic 5.4-derived kernel.
4. Confirm the eARC ALSA capture device and mixer controls.
5. Advertise 2.0, 5.1, and 7.1 LPCM capability sets separately.
6. Record deterministic channel-identification signals from a TV.
7. Validate channel order and amplitude.
8. Repeat after underruns, cable reconnect, TV standby, source changes, sample-rate changes, and 1000 link resets.
9. Run a minimum 24-hour capture test with zero channel permutation.
10. Measure end-to-end latency, drift, discontinuities, XRUNs, and CPU load.
11. Feed captured PCM into Aurora without changing renderer or realtime contracts.
12. Produce a hardware evidence report before selecting the path for the product roadmap.

## Decision

Current ranking for immediate experimentation:

1. Khadas VIM3L native eARC capture.
2. IT6620BFN plus XMOS UAC2 bridge.
3. EP91A7E plus digital bridge.
4. Commercial eARC-to-Dante/AES67 reference products.
5. Analog extractor and ADC fallback.

Current ranking for a future production bridge remains undecided until VIM3L reproduction and supply-risk analysis are completed.
