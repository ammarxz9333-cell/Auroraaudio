# ADR 0018 - Linux Audio Architecture

Status: Accepted (Draft)

## Decision
Aurora will rely on Linux and ALSA as the hardware abstraction layer instead of implementing its own audio driver stack.

### Aurora responsibilities
- DSP engine
- Room correction
- Bass management
- Time alignment
- Channel routing
- Wireless synchronization
- Plugin framework
- Preset management
- Diagnostics

### Linux/ALSA responsibilities
- HDMI audio drivers
- USB audio
- I2S
- PCIe audio
- DMA
- Audio device enumeration

## Architecture
Hardware -> Linux Kernel -> ALSA -> Aurora Input -> Aurora DSP Pipeline -> Aurora Output -> ALSA Output -> DAC/Amplifier/Wi-Fi Nodes

## Open research
Do not freeze the HDMI/eARC architecture until a validated Linux-compatible, low-latency multichannel solution has been proven.