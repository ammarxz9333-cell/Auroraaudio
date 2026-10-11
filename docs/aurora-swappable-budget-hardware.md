# Aurora low-cost and fully replaceable (software-open) hardware v1

Status: **architecture + source-code/CI conformance only**, dated 2026-10-11. NOT a 7.1.4 working physical system and NOT a complete BOM. The owner's existing Lindy 38368 / SiI9437 eARC receiver is an allowed proprietary hardware *exception*. Do not add closed Denon/Samsung AVR, closed host DSP decoder or proprietary mandatory audio software.

## Fixed interfaces, interchangeable implementations

```text
Authorized TV / Fire TV HDMI -> TV eARC
  -> [INGRESS v1: existing Lindy SiI9437 captured by open Linux ALSA]
  -> [DECODER: Aurora FFmpeg for AC-3/DD+ channel beds; pinned Harletty +
      Omniphony only after independently proven native JOC/Atmos corpus]
  -> [DSP: one open Aurora 12ch S32_LE / 48000 PCM bus; optional CamillaDSP]
  -> [OUTPUT v1: ONE clock-coherent Linux ALSA 12+ channel device, exact board TBD]
  -> [AMPLIFICATION v1: 11 independently driven channels + 1 active subwoofer]
  -> [ENDPOINTS v1: first WIRED; detachable wireless ESP after physical clock pass]
  -> [MANUAL calibration / servos: separate control module; no phone app yet]
```

The entire Aurora *application/control/DSP software* must stay open source. Linux, ALSA, FFmpeg, the selected permitted open object stack and Aurora's own adapters must be independently replaceable and versioned. Raspberry Pi itself contains proprietary hardware/boot firmware; it is not an entirely open-hardware machine. Likewise, external HDMI source apps are not part of Aurora's open-source runtime.

## Preferred low-cost compute (non-exclusive)

**Raspberry Pi 5, 4GB**: reference Germany price **€120.60**, currently listed at Reichelt as available from **2026-10-27**, not an in-stock assumption. Source: https://www.reichelt.de/de/de/shop/produkt/raspberry_pi_5_b_4x_2_4_ghz_4_gb_ram_wlan_bt-359842

**Raspberry Pi 5, 2GB**: reference **€72.10**, currently out of stock; can be substituted only after DD+/JOC CPU/memory/thermal and ALSA underrun tests. Avoid 8GB/16GB unless repeatable workload evidence demonstrates it is required.

The €200 target is for the **compute/boot accessory stage**, not an entire 7.1.4 product. Power supply, cooling, microSD, cables and capture-interface adaptation must be priced. The owned Lindy is counted at €0 *incremental* cost, not asserted free on the open market. Total 7.1.4 price remains **unknown** until one precise 12ch DAC interface, 11 amps, safe power stages, actual speakers, active sub and enclosures are quoted.

## The expensive missing part is exactly ONE verified 12+ channel audio DAC

- **Strongest clean interface**: a **single USB Audio Class 2 output device** presenting 12 or 16 channels on the SAME clock, accessible via Linux ALSA, fixed 48kHz and S32_LE, with an openly auditable source/firmware or owner-maintainable open firmware build.
- **Research candidate**: XMOS [lib_xua](https://github.com/xmos/lib_xua) is published with source and UAC2 functions. An Aurora-specific open firmware image + one coherent TDM16 -> dual eight-output DAC boards would require PCB design, board bring-up and real firmware mapping. XMOS source library DOES NOT mean the miniDSP MCHStreamer Lite firmware is open, nor does it prove our custom hardware exists or works. External toolchain/license must be audited.
- **Commercial prototype only**: [miniDSP MCHStreamer Lite](https://www.minidsp.com/products/usb-audio-interface/mchstreamer-lite) lists USD 95 and Linux-compatible 24-channel TDM8 interface. It is **not a 12-channel analog DAC**, and its vendor firmware must not silently be presented as open. It is NOT the selected fully-open Aurora output implementation.
- **Rejected as final 7.1.4 interface**: Pi 5 RP1/I²S 8-channel board alone. [RP1 experimental TDM driver](https://github.com/DatanoiseTV/rpi5-rp1-audio-tdm) documents 8 channels on the header bus; more require separate physical/peripheral access and demonstrated coherent DMA and clocks. A separate 2026 bug report (https://github.com/raspberrypi/linux/issues/7584) reports physical channel errors after I²S XRUN recovery on DAC8x; don't treat logical ALSA channel count as electrical validation.
- **Rejected**: concatenating several unrelated USB stereo/7.1 dongles to make 12 outputs without common clock + measured synchronization. Two devices do not magically have shared PLL.

A source-based design candidate is preferred over a fake "proven cheap 16-output kit." A **verified affordable fully source-maintainable 12ch USB DAC board has NOT been chosen**. Do not buy one yet on the basis of this document.

## Machine-readable swappable modules

`config/aurora-hardware-modules-v1.json` defines exactly five hardware boundaries and the single common Aurora bus channel order, while **software** decoders/renderer retain the separate versioned contracts already maintained in `config/aurora-audio-component-contracts-v1.json` (currently PR #443).

```bash
python3 validation/hardware/aurora_swappable_hardware.py
python3 validation/hardware/aurora_swappable_hardware.py --set compute=generic-linux-arm64
python3 validation/hardware/aurora_swappable_hardware.py --set output=xmos-lib-xua-16ch-prototype
python3 validation/hardware/test_swappable_hardware.py
```

- Good profile: returns `compatible_design: true`, **always** `physical_ready: false` until independent physical acceptance actually exists; JSON lists blockers and minimum **known-only** incremental component prices. Never mistake a lower bound for total system price.
- Wrong channel count/format/rate, independent output clocks, unapproved proprietary eARC hardware, wrong output boundary or insufficient amplifier feeds: **hard failure before swapping**.
- `--require-physical` must exit nonzero. The checker never rewires hardware, flashes firmware, hot swaps active audio, or authorizes playback itself.
- A part is replaceable at a **controlled shutdown / service event** (freeze old profile -> mute and stop -> physically switch -> ALSA/channel/clock and electrical validation -> soak -> accept or rollback). A powered amp, DAC or digital clock circuit is **not hot-unpluggable during playback** just because its software interface is replaceable.

## Release blockers (not optional)

1. Record actual authorized TV eARC AC3/DD+ PCM capture and accurately detect true JOC instead of assuming every EAC3 is Atmos. MAT, TrueHD, DTS:X and AC-4 are **not** yet universal physical input capabilities.
2. Demonstrate one affordable, source-maintainable UAC2 12ch+ output with all channels electrically mapped and a single observed sample clock.
3. Wire 11 independent amplifiers, 1 active sub and 7.1.4 speaker topology. Include supplies/fusing/thermal monitoring and physically confirm each speaker wire.
4. Prove 60 min of streaming + channel switches with zero unhandled XRUNs, bounded latency, appropriate ASRC across input/output clocks and repeatable error recovery.
5. Only then test detachable wireless/ESP modules, servo feedback and room auto-calibration.

**No fake compliance:** Open-source libraries running on a Raspberry Pi and 12-channel dummy data do not imply Dolby Atmos licensing, complete Netflix compatibility, eARC MAT reception or real 7.1.4 speakers.

The first deliverable in this PR is a reusable validation boundary to prevent future expensive incompatible component purchases. It does not falsely claim the missing board is now solved.
