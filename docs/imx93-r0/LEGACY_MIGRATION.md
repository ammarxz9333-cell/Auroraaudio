# Legacy Aurora Migration into i.MX93 R0

Aurora accumulated several useful architectures before the i.MX93 path was selected. R0 preserves the evidence and software that still helps, while retiring unnecessary hardware complexity.

## Retained

- Rust workspace, CLI, diagnostics, simulation and measurement infrastructure.
- The rule that simulated/software evidence must not be described as physical validation.
- Existing multichannel WAV/channel-role tooling.
- Startup/mute-first safety thinking and explicit acceptance gates.
- Legacy validation reports as historical evidence.
- External Vibesbox evidence for SiI9437 eARC/I2S capture behavior.
- Harletty + Omniphony as the current object-decode/render path.

## Retired as the default R0 hardware

- Intel N100 + STM32H753 split-compute architecture.
- Raspberry Pi 5 / CM5 as the production compute target.
- miniDSP MCHStreamer as the normal multichannel output bridge.
- ADAU1466 as a mandatory DSP/ASRC stage.
- separate PCM1690/CS42448 output chains as the default DAC topology.

These remain fallback/reference ideas only.

## New R0 replacements

| Legacy function | R0 replacement |
| --- | --- |
| Linux host + real-time MCU split | single i.MX93 module; M33 optional, not required initially |
| Pi I2S eARC capture | i.MX93 SAI1 RX |
| USB multichannel bridge | native i.MX93 SAI3 TDM512 |
| multiple unsynchronized DAC buses | two AK4458 in one TDM512 daisy chain |
| ad-hoc IEC extraction scripts | tested Rust extractor inside `aurora-audio-io` |

## Historical documents

The old reports remain useful because they contain previous failure analysis, validation logic, tooling and measurements. Their component choices are not authoritative for this branch when they conflict with `PROJECT_EXECUTION_STATE.md` or `docs/imx93-r0/*`.
