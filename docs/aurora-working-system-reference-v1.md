# Aurora working-system reference v1 — licensed AVR audio core

Decision (2026-10-11): **do not block real listening on experimental HDMI/eARC object decoding, Harletty integration, or DIY HDCP/EDID reverse engineering.** The first complete audio system uses a commercially supported licensed AV receiver for the entire HDMI/eARC -> Atmos/DTS:X -> amplified channels path. Aurora's detachable speaker positioning and motor controller run as independent control functions, without inserting a non-validated PCM pipeline into the audible path.

## Exact reference signal path

```
Fire TV Stick 4K Max / TV streaming apps / authorized Blu-ray player
  -> Denon AVC-X3800H HDMI input (preferred for external streamer), or TV eARC
  -> Denon licensed format decode / object rendering / Audyssey speaker calibration
  -> Denon 9 amplified channels + 2-channel external power amplifier via PRE OUT
  -> eleven individually identified loudspeaker feeds (7.1.4) + active subwoofer

Aurora controller (separate low-voltage control circuit)
  -> user-only recalibrate command -> position controller -> detachable speaker motor(s)
  -> encoder / limit / settling feedback -> no motion during audio calibration
```

The 7.1.4 mapping is FL/FR/FC/LFE/SL/SR/SBL/SBR/TFL/TFR/TRL/TRR. The LFE is an **active subwoofer** output, not one of the eleven speaker amplifiers. Front/rear height positions may use purpose-built Dolby Atmos enabled upfiring modules, with a ceiling/reflection-dependent result rather than guaranteed overhead acoustics.

## Parts required to produce audible 7.1.4

- Licensed processor/AVR: **Denon AVC-X3800H (EU model)**. Vendor documented: Dolby Digital/Plus, TrueHD/Atmos, DTS/HD/DTS:X, 11-channel processing, 9 internal amps, eARC, Audyssey. Not universal AC-4 / every future codec.
- Two-channel external **power amplifier** connected to the pair of PRE OUT channels assigned in Denon Amp Assign 11.1ch; verify channel routing on the AVR screen, not by guessing the terminal labels. This amplifier must accept the AVR's variable-level preamp signal with compatible gain.
- Seven main/surround speakers, four height/Atmos-enabled speakers, **one active subwoofer** with suitable power and impedances; eleven passive speaker runs in the first accepted build.
- HDMI eARC-capable TV, HDMI cables that meet equipment needs, existing compatible Fire TV Stick 4K Max; calibration mic supplied/compatible with AVR.
- Use conventional speaker cables for the first acceptance build. **Aurora's custom wireless detachable rear modules are NOT product-ready**; adding a wireless amp/link requires real latency, clock, power, dropout and synchronization validation. HEOS multiroom is not a drop-in 7.1.4 wireless rear interface.

## First working setup and tests

1. Wire receiver HDMI MONITOR1 (eARC) to TV's eARC port. Connect external Fire TV to receiver HDMI IN to avoid the TV's audio passthrough limitations where possible. Source settings: prefer bitstream/auto audio as supported; inspect receiver's actual incoming signal.
2. In Denon Speaker Layout / Amp Assign, select 11.1ch and correct 7.1.4 speaker topology; assign two channels to the external amp's PRE OUT. Configure one active LFE sub.
3. Use AVR's internal test tones for all 11 speaker channels, one at a time. Validate correct left/right, side/back, height mapping and speaker polarity. Record pass/fail.
4. Run Audyssey calibration with motorized speakers physically locked. After any change of speaker orientation, rerun calibration or a documented validated recalibration process. Aurora's synthetic auto-aim alone cannot authorize moving the speakers and assuming the AVR room model remains correct.
5. Test Dolby Digital Plus/Atmos from a stream known to deliver it; separately test TrueHD Atmos and DTS:X from a permitted disc/file source and check receiver input information. A display of 'Dolby Surround' / upmix mode alone does not prove native Atmos objects.
6. Check eARC from the TV and HDMI-direct input independently, source changes, power cycles, CEC, lip sync, 60+ minute continuous playback, heat, channel dropouts and speaker fault handling. Hardware/room acceptance must be logged before claiming an actually working Aurora home system.

## Aurora ownership and strict exclusions

The AVR, not Aurora, decodes and renders **all audible media** in the reference v1 audio mode. Aurora does not intercept protected HDMI content, duplicate Dolby object rendering, inject PCM between AVR and amps, or control consumer AVR DSP via undocumented vendor internals. Existing experimental Rust/FFmpeg/Harletty software branches remain **laboratory alternatives**, not required runtime dependencies of this working reference.

The Aurora audio-core R&D branch may independently progress to verified decoded 12ch PCM and wireless outputs, but must never be selected automatically in a consumer reference deployment before its own end-to-end physical gates. Mobile app is deferred until manual on-demand calibration, measured encoder feedback and genuine physical motor safety are validated.

## Explicit remaining blockers for original Aurora vision

- A modular multi-channel DIY front bar with individually removable/rotating speakers and reliable wireless surrounds **does not exist yet** as an accepted Aurora product; this AVR reference is a working audio subsystem architecture, not a claim that speaker enclosures/motor boards have been built.
- Receiver speaker feed over network and digital access to raw decoded 12ch Objects are not provided here. The AVR stays in the audio signal path. If Aurora must do independent object rendering on custom wireless units, this architecture will not meet that requirement without additional verified hardware/software engineering.
- AC-4 and some niche formats may require source-device PCM conversion or are not supported; never promise literally every codec.
- Physical build completion, bill of materials for room-specific speakers/amps, cost, and actual service tests are not established by this document.

## Source evidence

- Manufacturer capabilities: https://www.denon.com/de-de/product/archive-av-receivers/avc-x3800h/AVCX3800H.html
- Denon supported HDMI audio: https://manuals.denon.com/AVCX3800H/EU/DE/GFNFSYdtphpnek.php
- Denon Amp Assign/speaker wiring: https://manuals.denon.com/AVCX3800H/EU/EN/DRDZSYhylpohod.php
- Eleven-channel processing with external two-channel amplifier: https://www.denon.com/on/demandware.static/-/Library-Sites-denon_europe_shared/default/dwe49e7c14/downloads/Denon_AVC-X3800H_InfoSheet_EU.pdf

Status: **design/verified vendor capability; no physical assembly or product acceptance performed in this repository.**
