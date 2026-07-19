# eARC Acquisition Paths for Aurora

## Status

- research state: ACTIVE
- implementation state: NOT_STARTED
- purpose: define practical paths for receiving TV eARC/ARC audio as Aurora-owned multichannel PCM
- production claim: none until hardware measurements pass

## Core conclusion

Aurora should not attempt to implement the eARC physical/link layer on a generic Raspberry Pi. The practical near-term path is to use a dedicated eARC extractor that converts TV return audio into a conventional HDMI audio stream, then capture that stream with hardware that exposes up to eight embedded PCM channels to Linux.

## Preferred V1 architecture

```text
TV eARC/ARC port
    -> dedicated eARC extractor
    -> HDMI audio-only output carrying LPCM 2.0/5.1/7.1
    -> PCIe HDMI capture interface with 8-channel embedded-audio support
    -> Linux vendor SDK or capture API
    -> Aurora-owned bounded PCM blocks
    -> channel-map validation
    -> renderer / DSP / local and wireless outputs
```

### Initial candidate hardware

1. eARC extractor class
   - OREI HDA-927 or a functionally equivalent device that explicitly accepts TV eARC/ARC and emits an HDMI audio output supporting LPCM 5.1/7.1.
   - Higher-bandwidth alternatives may be evaluated for 4K120/8K systems, but video bandwidth is irrelevant when the device is used only as an audio-return converter.

2. HDMI capture class
   - Blackmagic DeckLink Mini Recorder 4K or a functionally equivalent PCIe capture interface.
   - Required properties: Linux support, documented SDK, HDMI input, and eight embedded HDMI audio channels.

This pairing is the first concrete path that avoids writing the eARC protocol while still giving Aurora access to uncompressed multichannel PCM.

## Why the path is credible

- Dedicated extractors already terminate the eARC/ARC link and advertise multichannel formats to the TV.
- The extractor's audio-only HDMI output behaves like a normal HDMI source toward the capture interface.
- Professional capture interfaces already ingest eight embedded HDMI audio channels and provide Linux drivers/SDKs.
- Aurora therefore owns only the capture adapter, channel semantics, buffering, timing, diagnostics, and downstream processing.

## Mandatory operating mode

Aurora V1 must request or require PCM from the television/source.

Accepted first-release formats:

- PCM 2.0
- LPCM 5.1
- LPCM 7.1
- 48 kHz preferred
- 24-bit or 16-bit samples converted into Aurora's canonical format

Not accepted as a first-release dependency:

- Dolby TrueHD decoding
- Dolby Digital Plus decoding
- DTS-HD decoding
- DTS:X decoding
- proprietary object metadata recovery from licensed bitstreams

An extractor may pass these formats, but Aurora must not claim it can decode them. The TV or source must output LPCM for the first implementation.

## Capture-adapter boundary

Create an isolated adapter behind Aurora-owned contracts. Vendor types must not enter renderer, DSP, configuration, transport, or CLI public APIs.

```text
capture SDK callback / polling thread
    -> fixed-capacity native staging buffers
    -> validated sample rate and channel count
    -> explicit channel-order conversion
    -> bounded Aurora PCM queue
    -> realtime engine
```

Rules:

- no allocation, logging, JSON, filesystem access, or device discovery in callback-reachable code;
- no assumption that capture-card channel order equals Aurora semantic order;
- capture timestamps and discontinuity counters must be retained;
- all unsupported formats must fail explicitly rather than silently downmixing;
- reconnect and format-change handling belong to the control plane;
- a software-only fake capture backend is required before physical integration.

## EDID and channel negotiation

The eARC extractor and capture interface together form the advertised audio sink. Aurora must record:

- TV model and firmware;
- extractor model and firmware;
- extractor EDID mode;
- capture-card mode;
- source audio setting;
- reported channel count and sample rate;
- observed channel map.

The validation procedure must use per-channel spoken or tonal identifiers and must not trust labels alone.

## HDCP boundary

Capture hardware may refuse protected HDMI video. Aurora is interested only in the audio stream produced by the extractor, but HDCP behavior must still be tested with each topology. No design may depend on bypassing, stripping, defeating, or circumventing content protection.

If protected-source behavior prevents lawful capture in the selected topology, the configuration is unsupported and the project must fall back to a source-side PCM path or a licensed hardware platform.

## Latency model

The complete input latency is:

```text
TV audio pipeline
+ eARC link and extractor buffering
+ HDMI output framing
+ capture-card buffering
+ driver/SDK buffering
+ Aurora queue depth
+ renderer and DSP latency
+ output transport latency
```

Aurora must measure rather than assume every term. Required evidence:

- end-to-end impulse latency;
- p50/p95/p99 callback or dequeue interval;
- format-change interruption duration;
- reconnect time;
- one-hour and 24-hour drift;
- channel integrity;
- lip-sync offset relative to video;
- behavior after TV standby/resume;
- behavior when switching internal apps and HDMI sources.

## Alternative paths

### A. Source-side HDMI split before the TV

```text
source device
    -> HDMI 2.1 splitter/extractor
    -> TV video path
    -> HDMI audio-only path
    -> capture card
```

Advantages:

- avoids TV eARC behavior;
- may reduce latency and format instability;
- useful for consoles, PCs, and streaming boxes.

Limitations:

- does not capture audio from TV internal applications;
- every external source must route through the splitter;
- VRR/ALLM/4K120 compatibility must be verified.

### B. Commercial AVR or processor as front end

```text
TV eARC
    -> low-cost AVR / processor
    -> multichannel pre-out, USB, Dante, AES67, or HDMI monitor/capture path
    -> Aurora
```

Use only if the device exposes a lawful, stable multichannel digital or analog output. This is a fallback, not the preferred Aurora identity.

### C. Custom eARC hardware

A future product may use a dedicated HDMI/eARC transceiver such as a current commercial HDMI transceiver with eARC receiver capability and multichannel PCM interfaces. This requires:

- licensed HDMI adopter access and compliance work;
- HDCP licensing where applicable;
- high-speed PCB design;
- firmware and EDID implementation;
- regulatory and interoperability testing;
- substantial non-recurring engineering cost.

This path is inappropriate for Aurora V1 but is plausible for first-party hardware after product-market validation.

### D. ARC-only fallback

ARC can provide stereo PCM and selected compressed formats but cannot be treated as a general uncompressed 5.1/7.1 input. Aurora may support ARC stereo as a degraded capability, clearly reported as such.

## Implementation checkpoints

1. Build a fake eight-channel capture adapter and deterministic fixtures.
2. Add an isolated DeckLink-class adapter behind Aurora-owned PCM contracts.
3. Validate 2.0, 5.1, and 7.1 channel maps from a generated HDMI source.
4. Insert the eARC extractor and validate TV internal-app LPCM.
5. Run format switching, standby/resume, reconnect, latency, and 24-hour stability tests.
6. Compare at least two extractor models before selecting recommended hardware.
7. Publish a compatibility matrix by TV, extractor, capture interface, source, format, and firmware.
8. Only then mark the path hardware-validated.

## Stop conditions

Do not proceed to product integration when any of the following remains true:

- the TV cannot be forced to output LPCM;
- channel order cannot be proven;
- protected-source operation would require circumvention;
- latency or drift exceeds the accepted cinema budget;
- the capture SDK cannot provide bounded stable delivery;
- standby/resume requires manual reconfiguration;
- the extractor silently downmixes or changes sample rate.

## Decision

The current primary eARC solution is therefore not a Raspberry Pi eARC receiver. It is a modular hardware bridge:

```text
TV eARC -> eARC-to-HDMI extractor -> Linux 8-channel HDMI capture -> Aurora
```

This must be evaluated as the first physical input path after the current software-only runtime and evaluation checkpoints are complete.
