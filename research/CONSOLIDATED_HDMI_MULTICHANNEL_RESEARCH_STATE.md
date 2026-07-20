# Consolidated HDMI and Multichannel Research State

**Date:** 2026-07-20  
**Purpose:** Single continuation reference for Aurora HDMI/eARC ingestion, multichannel Linux integration, DAC/output hardware, and proven ready-made solutions. This file consolidates prior research so future work starts from unresolved gaps rather than repeating searches.

---

## 1. Aurora target architecture

```text
TV / media player / game console
    -> HDMI or eARC multichannel LPCM ingress
    -> Aurora/Linux processing
    -> multichannel output interface
    -> wired or network-distributed amplification
    -> loudspeakers
```

The central unresolved problem is not multichannel playback from Linux. That part has several workable solutions. The unresolved product-grade problem is receiving up to eight discrete LPCM channels from HDMI/eARC into an open, controllable Aurora processing path.

Aurora should initially target already-decoded LPCM. Dolby/DTS bitstream decoding is outside the initial scope because it introduces proprietary licensing, certification, firmware, and legal constraints.

---

## 2. Current conclusions at a glance

| Path | Status | Conclusion |
|---|---:|---|
| Raspberry Pi/Linux -> 8 analog outputs | SOLVED FOR PROTOTYPE | Audio Injector Octo, ezsound 6x8, miniDSP and other multichannel USB/TDM products can provide development output paths. |
| HDMI output from Raspberry Pi -> 8 analog outputs | SOLVED BUT NOT RELEVANT TO INPUT BLOCKER | X6000/X7000 and similar extractors work for some projects, but this is playback from a Pi, not capture from a TV into Linux. |
| TV eARC -> standalone 8-channel DSP/DAC | COMMERCIALLY SOLVED | miniDSP Flex HT/HTx accepts up to 8-channel LPCM over eARC and processes it internally. |
| TV/HDMI -> 8-channel digital professional transport | COMMERCIALLY AVAILABLE AT HIGH COST | Audiopraise VanityPRO provides up to 8-channel digital extraction, but is proprietary and not a low-cost Aurora foundation. |
| TV eARC -> Linux USB multichannel capture | NOT FOUND | No verified, affordable, class-compliant product found that exposes eARC LPCM as an 8-channel USB capture device to Linux. |
| HDMI receiver -> documented 8-channel I2S/TDM module sold to individuals | NOT FOUND | Reference architectures exist, but no accessible low-risk module has yet met documentation, availability, and legal requirements. |
| Product-grade open HDMI/eARC receiver subsystem | OPEN BLOCKER | Requires either an obtainable receiver module, licensed silicon/reference design, or a different architecture that avoids direct HDMI implementation. |

---

## 3. Proven or serious downstream multichannel output options

### 3.1 Audio Injector Octo

**Role:** Raspberry Pi TDM multichannel ADC/DAC development board.

**Established value:**
- Practical multichannel ALSA path.
- CS42448-based 6-input/8-output architecture.
- Useful reference for TDM slot handling, device-tree configuration, clocking, mixed-signal layout, and channel alignment.
- Does not solve HDMI/eARC input.

**Risks:**
- Driver quality and compatibility concerns on newer kernels.
- Limited maintenance and uncertain current availability.
- Must not become a permanent Aurora dependency without kernel, licensing, and reproducibility verification.

**Decision:** Research and prototype reference only.

### 3.2 ezsound 6x8

**Role:** Modern Raspberry Pi 5 multichannel interface based on TI PCM3168A.

**Established value:**
- 6 analog inputs and 8 analog outputs.
- Simultaneous capture and playback demonstrated by its developer.
- Used in a working CamillaDSP active-crossover/amplifier system.
- Audio-card-generated clock and electrical isolation from the Pi.
- PCM3168A has Linux ASoC support and remains a strong modern codec candidate.

**Important limitation:**
- Schematics, PCB files, and final permissively licensed source package were not yet publicly available at the time reviewed. The developer stated they would be released around fulfillment/shipping.

**Decision:** Best currently known Aurora prototype output candidate, but not yet a reproducible open hardware baseline.

### 3.3 PCM3168A

**Role:** 6-ADC / 8-DAC multichannel codec.

**Strengths:**
- Suitable channel count for Aurora development.
- Flexible serial audio modes including TDM/I2S-class operation.
- Mainline Linux codec support exists.
- Modern community implementation evidence exists through ezsound 6x8.

**Open checks:**
- Exact Pi 4/Pi 5 full-duplex clock topology.
- Long-duration xrun stability.
- Objective analog measurements on the specific board implementation.
- Final hardware license and released design package.

**Decision:** Leading codec candidate for an Aurora-owned output board.

---

## 4. Ready-made HDMI/eARC solutions reviewed

### 4.1 SupTronics X6000 / X7000

**Architecture:** HDMI LPCM extraction to eight analog channels, commonly using an HDMI receiver plus four stereo DACs.

**Positive evidence:**
- Used by Raspberry Pi and CamillaDSP hobby projects.
- Can provide 7.1 analog output when the source sends multichannel LPCM.
- Relatively inexpensive.

**Critical defect:**
- Multiple users reported channel-order changes after underruns or buffer faults.
- In an active crossover, channel permutation can damage loudspeaker drivers.

**Decision:** REJECTED as a dependable Aurora foundation. It may only be used for low-risk bench experiments with protection and automatic channel verification.

### 4.2 CYP AU-11SA-4K22 / Evolve II-4K class extractors

**Role:** Commercial HDMI to eight-channel analog extractor.

**Positive evidence:**
- Reports of stable operation after correct EDID configuration.
- More dependable than the inexpensive X6000-class products.

**Limitations:**
- Analog output only.
- No verified class-compliant 8-channel USB capture into Linux.
- No documented reusable I2S/TDM interface.
- Proprietary and relatively expensive.
- Feeding Aurora requires a second 8-channel ADC, causing D/A then A/D conversion, added latency, cost, and noise.

**Decision:** Benchmark or emergency prototype path only.

### 4.3 miniDSP Flex HT / Flex HTx

**Role:** Complete eight-channel home-theater DSP with HDMI eARC LPCM input.

**Verified capabilities from official documentation:**
- Accepts up to eight channels of lossless LPCM over HDMI eARC.
- Older ARC is limited to stereo.
- Requires a source and television capable of passing multichannel LPCM.
- Does not decode Dolby or DTS bitstreams.
- Provides bass management, routing, PEQ, crossover, gain, and delay processing.
- Flex HT provides eight analog outputs; Flex HTx adds eight analog inputs and balanced/unbalanced I/O.
- USB supports eight-channel PCM playback into the device, up to 96 kHz.
- Critically, processed outputs are not returned to the computer over USB.

**What it proves:**
- The exact TV eARC -> 8-channel LPCM -> DSP -> 8 outputs architecture is commercially feasible and reliable.
- eARC reception itself can be packaged in a compact processor without a conventional AVR.

**Why it does not directly solve Aurora:**
- It is a closed standalone processor.
- Its USB connection is an audio input/configuration interface, not an eight-channel capture output to Linux.
- Aurora cannot insert its own Linux DSP engine after the eARC receiver without analog recapture or hardware modification.

**Decision:** Best commercial architecture benchmark. Also a viable fallback product for users who want a finished non-open system, but not a reusable Aurora input module.

### 4.4 miniDSP Flex HTn

**Role:** Flex HT variant with Dante/AES67 networking.

**Potential significance:**
- Demonstrates a commercial bridge between multichannel DSP and network audio.
- May be useful as an architecture benchmark for Aurora network-distributed outputs.

**Required clarification:**
- Verify whether eARC-received LPCM can be routed to Dante/AES67 outputs in the exact desired topology and at what latency/licensing cost.
- Verify channel count, network clocking, API/control access, and whether it could serve only as a temporary ingress bridge.

**Decision:** HIGH-PRIORITY commercial bypass candidate for further investigation, but not assumed to be open or low-cost.

### 4.5 Audiopraise VanityPRO

**Role:** Professional/audiophile multichannel HDMI digital audio extractor.

**Verified headline capability:**
- HDMI 2.0-class extraction.
- Up to eight-channel 192 kHz / 24-bit PCM.
- Multichannel digital audio transport with jitter management and DSD support.

**Potential significance:**
- Unlike analog extractors, this product proves that stable eight-channel digital extraction from HDMI is commercially achievable.
- Its output format may provide a professional digital bridge into AES/SPDIF-capable interfaces.

**Limitations:**
- Proprietary, specialist, and expensive.
- Not a direct USB multichannel capture device.
- Output connector/protocol mapping and Linux ingestion chain require exact manual verification.
- HDMI version and video pass-through may be insufficient for modern 4K120/8K gaming use.

**Decision:** Strong digital-reference benchmark and possible lab ingress source, not an Aurora mass-market foundation.

### 4.6 ThenAudio / AVPro Edge SHARC-V2 class adapters

**Role:** HDMI/eARC routing adapter that preserves multichannel audio in HDMI/eARC form.

**What it does:**
- Helps connect HDMI sources and eARC-only audio products.
- Passes multichannel encoded/digital audio toward an eARC sink.

**What it does not do:**
- Does not expose eight discrete PCM channels to Linux, USB, I2S/TDM, ADAT, or AES.
- Analog output is limited to compatible two-channel PCM use cases.

**Decision:** Useful accessory when pairing an HDMI source with an eARC sink such as miniDSP Flex HT, but not an Aurora capture solution.

### 4.7 FeinTech AX310 and similar eARC adapters

**Role:** Connect HDMI sources or displays to eARC sound devices.

**Limitations:**
- Routes HDMI/eARC to another HDMI/eARC endpoint.
- Does not extract eight discrete channels into an open digital interface.

**Decision:** Not a capture solution; potentially useful only as an eARC transport accessory.

---

## 5. HDMI receiver/reference architecture research

### 5.1 ADV7611

**Decision:** Rejected for the current eight-channel target unless new primary evidence demonstrates an accessible eight-channel LPCM output architecture.

### 5.2 ADV7625 and Analog Devices HDMI Audio reference platforms

**Positive evidence:**
- Professional reference architectures demonstrate HDMI receiver/transceiver silicon connected to multichannel audio codecs through digital serial audio buses.
- Confirms technical feasibility of HDMI -> multichannel digital audio -> codec/DSP.

**Blocking issues:**
- HDCP provisioning and legal access.
- Firmware and initialization complexity.
- Component lifecycle and sourcing.
- Evaluation-platform cost and obsolete dependencies.
- Lack of a simple, documented module sold in prototype quantities to individuals.

**Decision:** Architecture evidence, not yet a practical Aurora solution.

### 5.3 NXP TDA1997x family

**Positive evidence:** Linux video-driver references exist.

**Unresolved:**
- A Linux video driver does not establish usable ALSA eight-channel capture.
- Audio output format, HDCP path, reference hardware, and current sourcing remain insufficiently verified.

**Decision:** Keep as a bounded candidate only if primary audio documentation is found.

### 5.4 Lontium, ITE, MacroSilicon and OEM HDMI receiver modules

**Current state:**
- Numerous marketplace modules claim 5.1/7.1, ARC/eARC, or I2S.
- Claims usually fail to distinguish bitstream pass-through, analog extraction, EDID advertisement, and accessible eight-channel LPCM.
- Documentation and HDCP/legal provisioning are commonly unavailable.
- Some Chinese manufacturers refused prototype sales because the purchaser was not a registered/licensed company.

**Decision:** Do not repeat generic supplier searches. Contact only vendors meeting a written requirement sheet and record every response.

---

## 6. Why common apparent solutions are insufficient

### 6.1 HDMI output is not HDMI input

A Raspberry Pi can output 7.1 LPCM over HDMI to an extractor. This proves a playback path only:

```text
Raspberry Pi -> HDMI -> extractor -> 8 analog outputs
```

Aurora needs:

```text
TV/source -> HDMI/eARC -> 8-channel capture -> Linux/Aurora
```

These are fundamentally different directions and driver models.

### 6.2 eARC adapters are not PCM capture interfaces

Products such as SHARC-V2 and AX310 make an HDMI source appear to an eARC sink. They do not expose PCM samples through USB or TDM.

### 6.3 USB DAC input is not USB capture output

A device advertising “8-channel USB audio” often means that a computer can send eight channels into it. For Aurora ingestion, the device must appear to Linux as an eight-channel recording/capture source, which is much rarer.

### 6.4 Analog extraction is not an ideal product architecture

HDMI -> DAC -> ADC -> Linux can work, but it adds:
- conversion latency;
- clock-domain complexity;
- noise and distortion;
- duplicated converter cost;
- unnecessary analog cabling;
- reduced ability to guarantee channel identity.

It remains acceptable only for prototypes or as a temporary validation path.

---

## 7. Best current architecture options

### Option A — Pure Aurora target

```text
TV eARC / HDMI LPCM
    -> documented multichannel receiver
    -> TDM/I2S or UAC2 capture
    -> Raspberry Pi 5 / Linux / Aurora
    -> ezsound 6x8 or Aurora PCM3168A board
    -> amplifiers
```

**Status:** Desired architecture, ingress component not yet found.

### Option B — Professional digital bridge

```text
HDMI source
    -> VanityPRO-class digital extractor
    -> AES/SPDIF multichannel interface
    -> Linux/Aurora
    -> multichannel DAC
```

**Status:** Technically plausible but expensive; exact digital interface and Linux capture chain require verification.

### Option C — eARC to Dante/AES67 bridge

```text
TV eARC LPCM
    -> Flex HTn-class processor
    -> Dante/AES67
    -> Aurora network receiver or distributed endpoints
```

**Status:** High-priority investigation. Could bypass the missing USB capture device, but may be closed, costly, licensed, and unable to expose the exact pre/post-DSP signal required.

### Option D — Analog validation rig

```text
TV/source HDMI
    -> stable 8-channel analog extractor
    -> 8-channel USB ADC
    -> Aurora/Linux
    -> 8-channel DAC
```

**Status:** Buildable now, inefficient, suitable for simulation-to-hardware validation rather than final product design.

### Option E — Use miniDSP as finished subsystem

```text
TV eARC LPCM
    -> miniDSP Flex HT/HTx
    -> 8 analog outputs
    -> amplifiers
```

**Status:** Working commercial fallback, but Aurora's Linux engine is bypassed.

---

## 8. Research queue — continue only from here

### Priority 1 — miniDSP Flex HTn / Dante bypass

- [ ] Obtain official Dante/AES67 routing documentation.
- [ ] Verify whether eARC LPCM can be placed directly onto Dante channels.
- [ ] Determine pre-DSP versus post-DSP network routing.
- [ ] Determine latency, sample rate, channel count, and clock-master behavior.
- [ ] Determine whether Linux can receive it using Dante Virtual Soundcard, AES67/RAVENNA-compatible software, or dedicated hardware.
- [ ] Determine licensing, recurring fees, and resale restrictions.

### Priority 2 — VanityPRO digital chain

- [ ] Obtain manual and exact output formats/connectors.
- [ ] Determine whether eight PCM channels are carried as four S/PDIF pairs, AES3, or another format.
- [ ] Identify a Linux-compatible multichannel digital capture interface.
- [ ] Establish total cost and latency.
- [ ] Verify HDCP behavior and source compatibility.

### Priority 3 — HDMI/eARC to USB capture

Search only for devices that explicitly state all of:
- [ ] USB Audio Class capture/recording endpoint.
- [ ] At least eight input channels visible to the host.
- [ ] Linux or class-compliant support.
- [ ] LPCM over HDMI/eARC, not encoded pass-through.
- [ ] Stable channel mapping and explicit sample rates.

Reject products that only offer USB playback, stereo capture, video capture with stereo audio, or vendor-specific Windows-only recording.

### Priority 4 — HDMI/eARC to ADAT/AES/MADI

- [ ] Search professional cinema, broadcast, measurement, and installation products.
- [ ] Prefer digital outputs with standard Linux-compatible capture hardware.
- [ ] Record video format limits, HDCP, EDID, latency, and price.

### Priority 5 — Open receiver hardware

- [ ] Locate exact ADV7625/reference schematics and audio-bus configuration.
- [ ] Determine HDCP/legal feasibility for a small commercial product.
- [ ] Search for legitimate modules using receiver silicon with documented eight-channel LPCM I2S/TDM output.
- [ ] Require sample sales to individuals and usable initialization documentation.

### Priority 6 — Prototype validation path

- [ ] Define a protected analog-loop prototype using a stable commercial extractor and eight-channel ADC.
- [ ] Add per-channel pilot tones or coded identification.
- [ ] Implement automatic channel-permutation detection before enabling amplifiers.
- [ ] Use relay muting and frequency-band protection for active crossover tests.

---

## 9. Anti-duplication rules

Do not repeat searches for:
- generic “HDMI 7.1 extractor” products;
- consumer extractors with only analog RCA outputs unless they have new measurable evidence;
- HDMI-to-eARC adapters that terminate only in another HDMI port;
- USB devices where “8-channel” means playback only;
- ADV7611 without primary eight-channel evidence;
- Audio Injector as though it solves HDMI ingestion;
- Raspberry Pi HDMI playback projects as evidence of HDMI capture.

Every new candidate must be recorded with:
- exact model and manufacturer;
- input direction and connector;
- output direction and connector;
- PCM channel count;
- capture versus playback semantics;
- sample rates and word widths;
- EDID and HDCP behavior;
- Linux integration;
- channel-order stability;
- price, availability, and individual-sale status;
- source quality and verification status;
- Aurora decision.

---

## 10. Current engineering recommendation

1. Keep **ezsound 6x8 / PCM3168A** as the leading prototype output path.
2. Treat **miniDSP Flex HT/HTx** as proof that the desired eARC LPCM architecture is viable and as a commercial benchmark.
3. Investigate **Flex HTn Dante/AES67 routing** first because it may provide the fastest all-digital bypass around the missing USB capture endpoint.
4. Investigate **VanityPRO plus a Linux digital capture interface** as the second all-digital lab path.
5. If neither path is practical, build a protected analog validation rig while continuing the open receiver search.
6. Do not design an Aurora HDMI PCB until HDCP, receiver initialization, component access, and licensing are explicitly understood.

---

## 11. Principal sources reviewed

- miniDSP Flex HT/HTx official product pages and manuals.
- Audiopraise VanityPRO official product page.
- AVPro Edge / ThenAudio SHARC-V2 official product page.
- TI PCM3168A product documentation and Linux ASoC support references.
- ezsound 6x8 project logs and Crowd Supply updates.
- Audio Injector documentation, mailing-list references, and Linux community discussions.
- Raspberry Pi, diyAudio, EEVblog, Zynthian, and Audio Science Review community reports used only as secondary evidence where primary documentation was unavailable.

Community reports remain PARTIALLY VERIFIED until reproduced or supported by authoritative documentation.
