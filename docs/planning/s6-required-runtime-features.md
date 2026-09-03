# S6 Required Runtime Feature Integration

Status: owner-authorized hardware-blocked software milestone.

## Milestone record

- execution_state: IN_PROGRESS
- evaluation_classification: none
- truth_sources: unit_test, deterministic_simulation, host_api_observation
- physical_measurement: required later for every hardware-dependent acceptance claim
- implementation_dependency_on_phase_2: no for the bounded software work below
- acceptance_dependency_on_phase_2: yes for live HDMI/eARC, physical latency, CEC hardware, wireless endpoints, room measurement, thermals, and speaker behavior

## Goal

Integrate the runtime features required for the Galaxy S6 + STM32H753 appliance path without changing codec ownership or claiming physical validation:

1. consume STM32 CLOCK_REPORT telemetry and drive Aurora's existing PI drift controller plus Rubato ASRC;
2. add post-render bass management, LFE trim, headroom, speaker high-pass protection, and linked peak limiting;
3. add bounded positive audio lip-sync delay with a control-plane update path;
4. reset decoder/render/DSP state on explicit source/discontinuity events and mark the first post-transition PCM period discontinuous; automatic same-rate codec-format transition detection remains outside this software proof until the real capture/HAL or decoder event boundary supplies it;
5. add a reusable remote-clock estimator contract for future wireless rear endpoints without adding networking;
6. replace the room-measurement placeholder with deterministic offline sweep/delay/level/RT60 analysis and time/level calibration derivation;
7. define a generic local control boundary that future CEC/UI adapters can drive for mute, master gain, standby, and lip-sync;
8. keep Dolby MAT outside the implementation: advertise no MAT capability, fail closed for unsupported transport, and retain an explicit future adapter/legal-review boundary.

## Non-goals / stop boundary

- no DRM or HDCP circumvention;
- no Dolby MAT decoder implementation;
- no proprietary codec source copied into Aurora;
- no live network/wireless transport;
- no claim that CEC works on physical hardware;
- no claim that automatic room calibration is physically measured;
- no claim that same-rate codec-format changes are detected automatically before the real capture/HAL or decoder event source is integrated;
- no change to canonical 7.1.4 channel order;
- no allocation, logging, filesystem access, process spawning, or blocking locks inside callback-reachable DSP processing;
- no automatic merge into a release or production branch while physical gates remain open.

## Protected contracts

- 48 kHz canonical processing rate;
- 7.1.4 channel order: FL FR C LFE BL BR SL SR TFL TFR TBL TBR;
- 40-frame S6↔STM32 PCM quantum;
- STM32 remains physical audio-clock owner;
- S6 remains decode/render/DSP owner;
- Omniphony remains IEC61937 streaming demux/render host;
- Harletty remains the optional E-AC-3 JOC/OAMD bridge;
- Aurora truth-source terminology remains unchanged.

## Acceptance criteria for this software milestone

- CLOCK_REPORT updates alter one shared ASRC ratio across all 12 channels and remain bounded by the existing drift-controller limits;
- postprocessor produces exactly 40 output frames per emitted period and preserves finite 12-channel PCM;
- bass-managed small-speaker channels are high-passed and redirected low-frequency energy reaches only the LFE/sub output;
- original LFE trim is independently configurable from redirected bass gain;
- linked limiter prevents absolute output above the configured ceiling;
- lip-sync delay is bounded and deterministic;
- an explicit DISCONTINUITY/source-reset event resets renderer/postprocessor state and marks the first returned PCM period DISCONTINUITY;
- automatic same-rate codec-format transition detection is an open integration item and is not counted as accepted by the current tests;
- remote clock estimator converges under deterministic ppm+jitter simulations without network code;
- measurement crate deterministically estimates delay, level, RT60, and derives channel time/level alignment from synthetic fixtures;
- unsupported immersive transports remain fail-closed and MAT is not advertised;
- S6 appliance CI plus workspace fmt/clippy/tests pass on the integration head.
