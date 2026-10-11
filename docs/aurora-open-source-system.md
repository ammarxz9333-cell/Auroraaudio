# Aurora open-source-only system — correction and acceptance plan (2026-10-11)

## Non-negotiable user requirements

Aurora is an **open-source DIY 7.1.4 audio system**, not a Denon, Samsung, JBL or any closed AVR presented as Aurora's decoder or amplifier. Do not silently switch the project to a commercial closed audio processor. The **entire Aurora-developed software stack** (decoder integration, renderer, DSP, I/O, synchronization and motor logic) must remain source-available under auditable open-source licenses. Hardware components are separate: the existing proprietary SiI9437/Lindy eARC receiver chip is a clearly disclosed *external hardware boundary*, not an open-source eARC circuit. If the user additionally requires all silicon, firmware and PCB design to be open source, there is **no accepted universal HDMI/eARC + protected streaming solution** today; do not claim otherwise.

### Open signal chain — target, not certified hardware completion

```
TV apps or authorized Fire TV -> TV eARC
 -> externally identified eARC receiver (existing Lindy 38368/SiI9437 experimental I2S tap)
 -> Linux ALSA I2S 2ch/192k IEC61937 capture (must verify on selected physical unit)
 -> ONE bounded Aurora IEC burst parser / runtime codec dispatcher
      AC-3 / E-AC-3 plain bed -> FFmpeg decoder
      E-AC-3 JOC / TrueHD Atmos / DTS:X when source & bridge are independently confirmed
          -> Harletty decoder bridge + Omniphony renderer (ONE paired ABI-versioned bundle)
      LPCM from separately verified physical ingress -> native Aurora PCM lane
      MAT, AC-4, MPEG-H, IAMF, xHE-AAC etc. -> explicitly unsupported until an exact
        compatible capture transport, licensed/OSS bridge if needed and proven output exist
 -> exactly ONE 12-channel 48k S32LE Aurora PCM bus
 -> existing audio DSP / optional CamillaDSP room processing
 -> one master clock + bounded ASRC where domains differ
 -> validated 12-channel DAC interface + amplifiers -> physical 7.1.4 speakers
 -> optional detachable/wireless speaker endpoints only after synchronization pass
 -> manual-only microphone/servo calibration after physical testing; phone app last
```

No HDMI/HDCP decryption, circumvention or false claim of universal protected-service app compatibility. TVs/streaming players are external content sources with their own proprietary software; those are not part of Aurora's source tree.

## Reuse without duplicate runtimes

- **FFmpeg**: one ordinary AC-3/E-AC-3 bed decoder/PCM conversion engine.
- **Harletty + Omniphony**: exactly one mutually compatible set for object content, when independently enabled and verified. New upstream versions mentioning raw TrueHD/MAT and DTS:X are candidates, not automatic proof of Aurora end-to-end capture or hardware playback. Do not double-render 3D objects with an extra Aurora VBAP pass.
- **Aurora Rust engine**: PCM transport, one clock-domain-crossing ASRC, 7.1.4 channel contract, endpoint control and update supervisor.
- **CamillaDSP**: optional single active room EQ layer, not a second concurrent EQ/routing engine.
- **ESP firmware / motor calibration**: open-source Aurora-owned functions, gated behind microphone measurements, encoder safety, RF/sync acceptance.

This is *not* a new collection of applications. External inventories and alternative decoders are offline references only. Only pinned, license-audited source and tested binaries may enter the selected runtime.

## Current truthful support and blockers

| Path | Evidence/status | Blocker before system-ready |
| --- | --- | --- |
| AC-3 and DD+ 5.1 -> canonical 12ch | Actual FFmpeg encoded software fixtures and successful Streaming Surround 7.1.4 CI | Legitimate 2026 TV/eARC playback capture, real 12ch DAC/speaker probe, soak testing |
| 7.1 signaled LPCM -> 12ch | Raw-file software integration, not eARC physical multichannel ingress | A physical PCM receiver with verified hardware channel order |
| Native DD+ JOC -> actual object 7.1.4 | Pinned Harletty/Omniphony fixture lane | Proven JOC detection and actual streaming title/TV eARC ingress |
| TrueHD Atmos, DTS:X, raw/MAT bridge | Upstream candidate capabilities; no Aurora physical acceptance | Exact matched ABI, format-specific corpus, source transport, object/channel evidence |
| Dolby MAT over HDMI/eARC | No accepted Aurora physical MAT capture nor MAT metadata demux | Hardware carrier format and complete matching input path |
| AC-4/MPEG-H/IAMF/xHE-AAC | Research only | Real runnable input/decode/output proof |
| 12ch custom speaker amplifiers, wireless rears, moving modules | Lab controls/simulation, no whole-system physical acceptance | Electronic & acoustic prototypes, synchronization, power/safety tests |

Existing SiI9437 physical DD+ capture experiments do NOT prove universal eARC HBR. Never promote a software test to a Netflix, HDMI/eARC, lossless DTS:X or Atmos-perceptual acceptance badge.

## Critical path: do this in order, no phone app or speculative add-ons

1. Finish CI-green **PR #443** on the existing branch. Preserve proven AC-3/DD+ + raw LPCM and fail-closed packet validation.
2. Run the existing *paired Harletty/Omniphony candidate CI*, freeze only the ABI-compatible artifact proven there, and create/extend format-specific fixture tests for **real** E-AC-3 JOC, TrueHD and DTS:X object metadata; label outcome software-only.
3. Add a production routing policy that picks native object decode ONLY on positive codec+object verification; plain DD+ uses its bed path. In unknown cases, keep muted or explicitly permit an operator-chosen **labeled** bed fallback; never fake Atmos.
4. Validate selected legitimate eARC receiver on real TV/Fire TV streams, including AC-3, DD+ and if supported JOC, source changes/reconnect, drift and burst defects. Do not infer MAT from LPCM.
5. Validate a specific Linux ALSA 12ch DAC interface + amp stages and 7.1.4 speaker mapping **electrically**, then 1h continuous playback with bounded latency/XRUN telemetry. Bare PCB plans alone cannot pass.
6. Add detachable wireless synchronized endpoints and motorized on-demand calibration only after wired playback is repeatably successful. Maintain individually upgradeable modules and stable rollback.

## Release definition

"Aurora 7.1.4 open-source system works from TV apps" requires a **recorded, repeatable physical test** of source -> authorized eARC -> object/bed decoder -> synchronized 12ch PCM -> DAC -> eleven speakers + subwoofer. Document exact source codec, whether height channels are native or synthesized, content/transport evidence, 7.1.4 electrical channel map, failure recovery and no-dropout soak duration. A continuous 12-channel buffer alone is never a product pass.

**Design rule:** neither an off-the-shelf closed AVR processor nor a Windows/REAPER dependency may be the required Aurora playback core. Commercial HDMI/eARC receiver silicon must be disclosed and never represented as open hardware.

**Upstream OSS references**
- https://github.com/mgth/Omniphony (GPL-3.0 spatial renderer)
- https://github.com/harletty/harletty-bridge (source available, bridge plugins GPL-3.0-or-later)
- https://github.com/FFmpeg/FFmpeg
- https://github.com/HEnquist/camilladsp
- https://github.com/sofianchitac/VibesboxSRC (useful hardware-tap reference only; overall DSP reference depends on Windows/REAPER and is not a ready full OSS 7.1.4 system)
