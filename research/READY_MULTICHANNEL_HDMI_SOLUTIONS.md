# Ready Multichannel HDMI Solution Evidence

**Last reviewed:** 2026-07-20  
**Scope:** Existing hardware that has been used successfully to obtain discrete multichannel LPCM from HDMI, with emphasis on real-world reliability and suitability for Aurora.

## 1. Executive conclusion

Two ready-made product families have credible real-world evidence:

1. **SupTronics X6000 / X7000** — low-cost 7.1 LPCM HDMI de-embedder with four stereo DACs. It demonstrably works, but repeated users reported channel-order swapping after underrun/overrun events. This is unacceptable as Aurora's dependable core path.
2. **CYP AU-11SA-4K22 / Evolve II-4K** — commercial HDMI multichannel de-embedder/DAC. A user reported stable operation after proper EDID configuration. It is more credible as a validation reference than the SupTronics board, but it is expensive, closed, and does not expose the extracted channels digitally to Linux.

No ready, low-cost, currently supported module was found that simultaneously provides:

- HDMI/eARC input,
- eight-channel LPCM,
- direct USB Audio Class capture or documented I2S/TDM output,
- Linux integration,
- open design files,
- and proven long-duration channel-order stability.

## 2. SupTronics X6000 / X7000

- **Status:** REJECTED as Aurora production input; acceptable only for controlled experiments.
- **Category:** HDMI repeater / 7.1 LPCM analog audio extractor.
- **Primary architecture reported by users:**
  - EP91A6S HDMI receiver/repeater.
  - Four ESS ES9023 stereo DACs.
  - Eight analog output channels.
- **Accepted input:** Multichannel LPCM. It does not decode Dolby TrueHD or DTS-HD MA itself. The source must decode these formats to LPCM first.
- **What was proven:**
  - Multiple users obtained working 7.1 analog output.
  - It was used with Raspberry Pi, Moode, ALSA, and CamillaDSP-based crossover systems.
  - A completed digital crossover/amplifier ran with the X7000 before being replaced by a custom sound card.
- **Critical defect:**
  - Multiple independent reports describe channel-order swapping after an underrun or buffer event.
  - The fault can occur after hours of operation.
  - Restarting CamillaDSP is not a deterministic cure.
  - The issue persisted across Raspberry Pi generations and newer Linux releases in user tests.
- **Aurora consequence:**
  - Wrong channel order in an active crossover can route low frequencies to tweeters or high frequencies to woofers and can damage loudspeakers.
  - Therefore this path fails Aurora's deterministic channel-order requirement.
- **Availability:** Legacy/discontinued or difficult to obtain depending on revision and region.
- **Decision:** Do not buy as the architecture baseline. Only use an already-owned unit for fault-reproduction testing or to validate source-side LPCM/EDID behavior.

## 3. CYP AU-11SA-4K22 / Evolve II-4K

- **Status:** PARTIALLY VERIFIED.
- **Category:** Commercial HDMI multichannel audio de-embedder / DAC.
- **What was reported:**
  - A user in the Raspberry Pi multichannel DSP community reported that the Evolve II-4K worked properly after adjusting its EDID settings.
  - The same user contrasted it with X6000 problems and later moved to USB output with a Topping DM7.
- **Strengths:**
  - Commercial AV product rather than an undocumented hobby board.
  - User-configurable EDID appears to make source negotiation more predictable.
  - Practical analog multichannel output.
- **Weaknesses:**
  - Higher cost.
  - Closed hardware and firmware.
  - No established USB Audio Class capture path into Linux.
  - No established public I2S/TDM interface suitable for direct Aurora DSP ingestion.
  - Independent long-duration measurements and failure-injection tests were not located.
- **Aurora consequence:**
  - Useful as a reference or temporary analog-input prototype source.
  - Not a clean product architecture because Aurora would have to reconvert eight analog channels through ADCs, adding latency, noise, cost, and clock-domain complexity.
- **Decision:** Keep as a benchmark candidate only. Do not adopt unless a used unit is available cheaply and the goal is near-term proof-of-concept, not final architecture.

## 4. Old modular ARC receiver project

- **Status:** REJECTED as a ready solution; retained as historical architecture evidence.
- **Project:** Modular Open-Source AV Receiver.
- **Hardware:** EP91H0 ARC receiver plus CS8416 S/PDIF decoder.
- **Limitation:** Classic ARC/S/PDIF bandwidth does not provide general eight-channel uncompressed LPCM. The project was later abandoned/restructured and did not deliver a complete ready platform.
- **Decision:** Do not treat it as a route to Aurora 7.1 LPCM.

## 5. Important distinction: HDMI output versus HDMI capture

Several successful Raspberry Pi systems send eight channels **out of a Raspberry Pi over HDMI** into an analog extractor. That proves HDMI multichannel playback, but it does not solve Aurora's harder use case:

```text
TV / external HDMI source
    -> capture discrete eight-channel LPCM into Linux
    -> Aurora DSP
```

A normal Raspberry Pi HDMI connector is an output, not a multichannel HDMI capture input. Consumer HDMI capture dongles usually capture stereo audio or compressed/video-oriented streams and are not evidence of eight-channel LPCM ALSA capture.

## 6. Current recommended prototype routes

### Route A — Lowest technical risk for Aurora DSP development

```text
PC / Raspberry Pi media source
    -> software-decoded 7.1 PCM
    -> Aurora / CamillaDSP
    -> ezsound 6x8 or proven multichannel USB DAC
```

This avoids HDMI capture while completing and validating Aurora's DSP, calibration, routing, and output subsystems.

### Route B — TV integration proof-of-concept

```text
TV or player configured for 7.1 LPCM
    -> known commercial HDMI multichannel analog extractor
    -> 6/8-channel ADC interface
    -> Aurora DSP
```

This is inefficient but testable. Use only to validate television EDID behavior, lip sync, source compatibility, and control flows while the digital HDMI/eARC receiver remains unresolved.

### Route C — Product-quality target

```text
eARC / HDMI receiver with documented 8-channel I2S/TDM
    -> Aurora-controlled clock / ASRC boundary
    -> Linux capture or FPGA/MCU bridge
    -> Aurora DSP
```

This remains the correct research target.

## 7. Purchase gate

Do not purchase a multichannel HDMI extractor for Aurora unless all of the following are established:

- Explicit acceptance of eight-channel LPCM, not only Dolby/DTS pass-through.
- Exact channel map.
- EDID configuration method.
- Behavior during source sample-rate changes.
- Behavior after deliberate underrun/overrun or HDMI hot-plug events.
- Recovery behavior without manual reboot.
- Current availability and return rights.
- Whether output is analog only or exposes documented digital audio.

## 8. Next bounded research task

Search only for devices or modules that expose extracted HDMI/eARC audio as one of:

1. **USB Audio Class 2 multichannel capture**, or
2. **documented eight-channel I2S/TDM**, or
3. **ADAT/AES67/Dante-compatible digital output** that Linux can capture through obtainable hardware.

Exclude analog-only extractors unless they are materially cheaper and used solely as temporary validation hardware.
