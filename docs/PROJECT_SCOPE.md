# Aurora project scope

Aurora is a hardware-agnostic immersive-audio software project.

## In scope

- audio channel/layout and scene models;
- object and channel metadata boundaries;
- decoder adapter APIs;
- loudspeaker and binaural rendering;
- DSP, calibration, gain, delay, bass management, and output safety primitives;
- realtime scheduling, buffering, ASRC, drift control, latency accounting, and fault recovery;
- generic local audio I/O;
- deterministic simulation and software validation;
- optional external integrations through documented adapter interfaces.

## Out of scope for the core repository

- phone-specific boot images, rooting, firmware, kernel fragments, or Android probes;
- board-specific MCU firmware and pinmux/HAL contracts;
- a mandatory HDMI/eARC capture board;
- a mandatory USB/TDM/DAC/amplifier chain;
- a fixed commercial soundbar-like bill of materials;
- DRM circumvention or extraction of protected media;
- claims of proprietary-format certification without the corresponding licensed implementation and evidence.

A future hardware implementation can consume Aurora through adapters, but no named device is the canonical Aurora target.
