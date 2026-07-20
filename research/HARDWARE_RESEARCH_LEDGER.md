# Aurora Hardware Research Ledger

**Status:** Active research control document  
**Primary purpose:** Prevent repeated research, separate verified facts from hypotheses, and maintain a precise queue of unresolved hardware questions for Aurora.  
**Applies to:** HDMI/eARC input, multichannel PCM capture, TDM/I2S transport, multichannel codecs/DACs, Raspberry Pi audio interfaces, OEM modules, and related licensing constraints.

---

## 1. Mandatory research protocol

Before starting any new hardware search:

1. Read this file.
2. Check the **Reviewed components and projects** table.
3. Check the **Rejected or insufficient paths** section.
4. Search only for an unresolved item in the **Open research queue**.
5. Add every material result here, including negative results.
6. Mark each claim as one of:
   - **VERIFIED** — supported by a primary source, schematic, datasheet, repository, or direct test.
   - **PARTIALLY VERIFIED** — supported, but an important implementation detail remains unresolved.
   - **UNVERIFIED** — plausible lead that still requires primary-source confirmation.
   - **REJECTED** — unsuitable for Aurora under the current requirements.
7. Never promote a consumer listing, forum statement, or marketplace claim to VERIFIED without primary evidence.

---

## 2. Current system objective

Aurora needs a low-cost and dependable path for receiving television or media-player audio and delivering discrete multichannel PCM into the Aurora processing pipeline.

Target signal path:

```text
HDMI/eARC source
    -> legal and stable audio receiver/extractor
    -> discrete LPCM channels
    -> Linux/Aurora DSP pipeline
    -> multichannel DAC or distributed network outputs
    -> amplification and loudspeakers
```

Current preferred baseline for development:

```text
Linux host / Raspberry Pi
    -> TDM multichannel audio interface
    -> 8-channel analog output
    -> amplifiers
```

The HDMI/eARC input stage remains a separate unresolved blocker.

---

## 3. Non-negotiable requirements

- At least 8 discrete output channels for 7.1-class development.
- Stable channel ordering.
- Deterministic sample-rate and clock behavior.
- Linux integration suitable for automated testing.
- No dependence on undocumented consumer extractor behavior.
- No requirement to decode proprietary Dolby or DTS formats inside Aurora unless licensing is explicitly secured.
- LPCM input is preferred.
- Hardware must be obtainable in prototype quantities or reproducible from accessible design files.
- The final product path must not depend on abandoned, binary-only, or legally unclear software.

---

## 4. Reviewed components and projects

| Item | Category | Status | What is established | Remaining question / action |
|---|---|---:|---|---|
| Audio Injector Octo | Raspberry Pi multichannel audio HAT | PARTIALLY VERIFIED | Provides a practical Linux/Raspberry Pi path to multiple analog inputs and outputs. Relevant as a development platform and design reference. It does not solve HDMI input. | Confirm current availability, exact open-hardware artifacts, current kernel compatibility, and licensing of all reused design material. |
| CS42448 | 6 ADC / 8 DAC audio codec | VERIFIED at architectural level | Suitable for multichannel TDM audio conversion and historically associated with Audio Injector Octo-class designs. | Verify lifecycle status, current sourcing, Linux codec support, noise performance, clock topology, and whether a newer codec is preferable. |
| TDM on Raspberry Pi | Digital multichannel transport | VERIFIED at concept level | TDM is the correct class of interface for transporting multiple channels over a limited number of serial audio pins. | Establish the exact supported slot count, word width, clocks, DMA constraints, and stable kernel configuration for the selected Pi generation. |
| ADV7611 | HDMI receiver | REJECTED for current 8-channel target pending contrary evidence | Useful HDMI receiver family member, but not currently established as the correct low-risk path for direct 8-channel LPCM extraction into Aurora. | Do not revisit unless a primary-source reference design proves the required 8-channel output and obtainable implementation path. |
| ADV7625 | HDMI transceiver / receiver family | PARTIALLY VERIFIED | Stronger architectural candidate than ADV7611 for AVR/soundbar-style multichannel HDMI audio extraction. Evaluation/reference material exists. | Verify availability, cost, HDCP provisioning, firmware requirements, accessible schematics, output format details, and whether it can legally and practically be used in a small commercial product. |
| Analog Devices HDMI Audio EI3 EZ-Extender | Evaluation/reference platform | PARTIALLY VERIFIED | Demonstrates a professional HDMI-to-multichannel-audio architecture using HDMI receiver/transceiver technology and multichannel codecs. It is primarily a reference/evaluation path, not yet a cost-effective product solution. | Obtain and inspect complete schematics, BOM, firmware dependencies, licensing restrictions, and obsolete parts. |
| AD1939 / AD193x family | Multichannel audio codec | PARTIALLY VERIFIED | Relevant professional multichannel codec family used in reference audio architectures. | Compare against CS42448 and modern alternatives on cost, lifecycle, output count, Linux support, and measurable performance. |
| TDA19971 / TDA19973 family | HDMI receiver | UNVERIFIED for Aurora audio path | Linux video-driver references exist, but this alone does not establish a practical 8-channel audio extraction path for Aurora. | Confirm audio output capabilities, HDCP handling, accessible reference designs, component availability, and Linux audio integration. |
| Generic consumer HDMI audio extractors | Consumer appliance | REJECTED as core architecture | May be useful for temporary experiments, but cannot be treated as a dependable product foundation because channel mapping, EDID behavior, firmware, clocks, and internal topology are commonly undocumented. | Only evaluate a specific unit when internal chipset, output format, and repeatable measurements are available. |
| Raspberry Pi custom multichannel sound cards | Community/open projects | UNVERIFIED lead class | Potential source of reusable TDM, ALSA, device-tree, and PCB implementation knowledge. | Identify concrete repositories with schematics, source, license, recent maintenance, and test evidence. |

---

## 5. Audio Injector findings

### Established relevance

Audio Injector is relevant because it addresses the downstream half of the problem:

```text
Aurora DSP / ALSA
    -> TDM audio interface
    -> multichannel codec
    -> analog outputs
```

It is **not** an HDMI receiver, eARC endpoint, Dolby decoder, or Atmos decoder.

### Potential value to Aurora

- Multichannel ALSA topology.
- TDM slot configuration.
- Device-tree patterns.
- Codec clocking and reset sequencing.
- PCB layout reference for mixed-signal multichannel audio.
- Prototype platform for validating Aurora DSP, channel routing, calibration, and simulation-to-hardware correlation.

### Required verification before adoption

- Exact hardware revision and codec used.
- Whether complete schematics, PCB files, BOM, and fabrication outputs are available.
- License applying to hardware files and software.
- Compatibility with current Raspberry Pi models and current Linux kernels.
- Availability of boards and replacement components.
- Measured output noise, distortion, crosstalk, and channel consistency.
- Whether the driver is upstream, out-of-tree, abandoned, or dependent on old kernel APIs.

### Current decision

**Use as a research and prototype candidate, not yet as a production dependency.**

---

## 6. Architectural conclusions already reached

### 6.1 HDMI input and multichannel DAC are separate subsystems

An 8-channel DAC HAT does not solve HDMI or eARC ingestion. The project must not conflate these layers.

### 6.2 LPCM is the lowest-risk initial input target

Aurora should first accept already-decoded discrete LPCM. Proprietary bitstream decoding introduces licensing, certification, implementation, and legal constraints.

### 6.3 TDM is the preferred local digital bus

For eight or more channels between a host audio interface and codec, TDM is preferable to treating the design as multiple unrelated stereo I2S buses.

### 6.4 Reference designs are evidence, not automatically products

An evaluation board may prove feasibility while remaining too expensive, obsolete, restricted, or complex for a commercial Aurora device.

### 6.5 Driver availability must be evaluated separately from chip capability

A Linux video driver for an HDMI receiver does not prove that multichannel HDMI audio is exposed as a usable ALSA capture device.

---

## 7. Rejected or insufficient paths

### 7.1 Repeated generic searches for “HDMI 7.1 extractor”

**Reason:** Produces the same consumer products with undocumented internals and does not advance the architecture.

### 7.2 Treating Audio Injector Octo as the complete solution

**Reason:** It addresses multichannel ADC/DAC output, not HDMI/eARC reception.

### 7.3 Assuming a chipset works because a seller writes “7.1”

**Reason:** “7.1” may refer to compressed pass-through, analog output, EDID advertisement, or marketing rather than accessible eight-channel LPCM over I2S/TDM.

### 7.4 Assuming Linux kernel presence equals complete audio support

**Reason:** Kernel support may cover only video, control, or V4L2 functionality and still omit usable ALSA multichannel capture.

### 7.5 Re-contacting OEM suppliers without a qualified requirement sheet

**Reason:** Suppliers cannot provide a useful answer unless the exact input, output, sample rates, HDCP, EDID, MOQ, documentation, and licensing requirements are specified.

---

## 8. Open research queue

Research must proceed in this order unless a newly discovered blocker changes priorities.

### Priority A — Audio Injector evidence package

- [ ] Locate the authoritative Audio Injector Octo hardware repository.
- [ ] Record license for schematic, PCB, firmware, and driver separately.
- [ ] Identify exact codec and clock components by board revision.
- [ ] Locate current driver/device-tree source.
- [ ] Determine latest known working kernel and Raspberry Pi model.
- [ ] Find objective measurements or produce a measurement plan.
- [ ] Determine current purchase availability and price.

### Priority B — Raspberry Pi TDM capability matrix

- [ ] Pi 4 supported TDM slots, rates, and word widths.
- [ ] Pi 5 supported TDM slots, rates, and word widths.
- [ ] Master/slave clock options.
- [ ] DMA and long-duration stability evidence.
- [ ] Mainline kernel status.
- [ ] Known limitations with simultaneous capture and playback.

### Priority C — HDMI LPCM receiver candidates

For each candidate, collect:

- [ ] HDMI version.
- [ ] Maximum LPCM channel count.
- [ ] Audio output format: I2S, TDM, S/PDIF, or proprietary.
- [ ] Supported sample rates and word lengths.
- [ ] EDID control method.
- [ ] HDCP key/provisioning requirements.
- [ ] Firmware or microcontroller dependency.
- [ ] Public datasheet and reference schematic availability.
- [ ] Component lifecycle and prototype sourcing.
- [ ] Linux integration route.
- [ ] Legal/commercial restrictions.

Initial candidate list:

- [ ] ADV7625.
- [ ] Other obtainable Analog Devices HDMI receivers/transceivers.
- [ ] NXP TDA1997x family, only if audio capability is confirmed.
- [ ] Lontium parts with public documentation and legitimate sourcing.
- [ ] ITE Tech parts with accessible documentation and prototype support.
- [ ] MacroSilicon parts only where multichannel LPCM output is explicitly documented.
- [ ] FPGA HDMI receive approaches only as a fallback due to HDCP and complexity.

### Priority D — Existing OEM or development modules

- [ ] Search China, India, Taiwan, and small professional AV vendors.
- [ ] Require explicit 8-channel LPCM output over I2S/TDM or USB Audio Class.
- [ ] Require sample availability to individuals or unincorporated developers.
- [ ] Require documentation before purchase.
- [ ] Record every contacted vendor and response to prevent repeat outreach.

### Priority E — Modern multichannel codec selection

Compare at minimum:

- [ ] CS42448.
- [ ] AD1938/AD1939 or current successor.
- [ ] AKM multichannel DAC candidates.
- [ ] ESS multichannel DAC candidates where documentation and Linux integration are practical.
- [ ] TI multichannel codecs/DACs.

Comparison fields:

- channel count, price, availability, lifecycle, TDM flexibility, Linux support, clocking, analog performance, PCB complexity, and licensing/documentation access.

---

## 9. Supplier/contact ledger

No supplier should be contacted twice without first checking this section.

| Date | Company | Country | Product / chipset | Contact route | MOQ | Individual sale | Documentation received | Result | Next action |
|---|---|---|---|---|---:|---|---|---|---|
| TBD | Chinese manufacturers previously contacted by project owner | China | HDMI multichannel receiver/module, exact vendors not yet recorded | Direct inquiry | Unknown | Refused because buyer was not a licensed company | No | Closed until vendor identities and responses are reconstructed | Add names, dates, and exact replies from prior messages or email records. |

---

## 10. Evidence record template

Copy this block for every serious lead:

```markdown
### [Component / project name]

- **Status:** VERIFIED / PARTIALLY VERIFIED / UNVERIFIED / REJECTED
- **Category:**
- **Primary source:**
- **Secondary sources:**
- **What it claims:**
- **What is independently established:**
- **Audio input:**
- **Audio output:**
- **Maximum LPCM channels:**
- **Sample rates / word widths:**
- **Clocking:**
- **Linux support:**
- **Hardware files:**
- **Software license:**
- **Hardware license:**
- **HDCP / proprietary licensing:**
- **Availability / price / MOQ:**
- **Aurora fit:**
- **Blocking unknowns:**
- **Decision:**
- **Last reviewed:** YYYY-MM-DD
```

---

## 11. Immediate next investigation

The next research pass must focus only on **Audio Injector Octo primary evidence**:

1. Authoritative repository or downloadable design package.
2. Hardware and software licenses.
3. Exact codec and board revision.
4. Kernel and Raspberry Pi compatibility.
5. Current availability.
6. Reusable implementation details for Aurora.

Do not return to broad HDMI receiver searching until this evidence package is complete and recorded here.

---

## 12. Change log

### 2026-07-20

- Created the centralized hardware research ledger.
- Recorded known Audio Injector, CS42448, TDM, ADV7611, ADV7625, AD193x, TDA1997x, and consumer extractor conclusions.
- Added mandatory anti-duplication protocol.
- Added supplier ledger, evidence template, rejected paths, and prioritized research queue.
- Set Audio Injector primary-source verification as the next bounded research task.
