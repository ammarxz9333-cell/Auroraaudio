# Aurora eARC USB Bridge — software-only feasibility experiment

**Status: EXPERIMENTAL — NO HARDWARE VALIDATION.**

This is a removable platform-adapter experiment. Aurora's Rust core is hardware-agnostic and must not depend on eARC, STM32 or a specific USB vendor protocol.

## Purpose
A future eARC receiver (candidate Explore EP92H1) may expose 8ch LPCM, raw IEC 61937/HBR transport words and sideband audio status to an STM32H753, which could forward opaque payloads over a vendor-specific USB HS device interface to Raspberry Pi 5. A separate supported decoder/renderer can subsequently produce 12–16 PCM speaker outputs. Nothing in this directory implies a verified hardware receiver or Atmos capability.

## How to run
Run `python3 -m unittest discover -s experiments/earc-usb-bridge -p 'test_*.py' -v` from repository root, or from this directory run `python3 -m unittest -v`.

## Validation boundary
- The Python transport tests use synthetic bytes and intentionally do not decode/alter payloads.
- A separate local test with **real, non-Atmos E-AC-3 5.1** muxed to IEC61937 passed byte-for-byte equality and ffprobe identification. The codec sample was generated with ffmpeg, **not captured via eARC**.
- HBR lane order, four simultaneously synchronized SAI RX ports, STM32 pin multiplexing with USB ULPI, USB sustained throughput, proper eARC capability exchange and preservation of Dolby MAT/JOC metadata are **NOT VALIDATED**.
- Don't advertise unsupported formats to the source, and never silently downgrade or call 8-channel PCM equivalent to object metadata.

## Sources / hardware blockers
- Explore EP92H1 official page: https://www.epmi.com.tw/hdmi-arc-earc/ep92h1/ (HBR + IEC 61937 + AIF + channel status + U-bits documented at feature level only).
- STM32 ULPI HS reference: https://www.st.com/content/st_com/en/technical-documents/an4879-introduction-to-usb-hardware-and-pcb-guidelines-using-stm32-mcus-stmicroelectronics_asset/chunk-004-topic-an4879-otg-hs-phy-connected-through-ulpi.html
- Lattice SiI9437 alternative: https://www.latticesemi.com/Products/ASSPs/HDMI21eARC
- IEC 61937 format references: https://learn.microsoft.com/en-us/windows/win32/coreaudio/representing-formats-for-iec-61937-transmissions
- EP92H1E distributor: https://www.semiconn.com/ep92h1e . Do not assume EP92H1 and EP92H1E identical pinout.

**Hardware gate:** require exact silicon revision, register map, reference PCB, clock plan, real eARC MAT/TrueHD/JOC test vectors with a controlled transmitter and verified metadata preservation. **Do not fabricate PCB before this gate.**
