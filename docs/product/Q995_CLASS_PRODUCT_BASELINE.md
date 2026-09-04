# Aurora Q995-Class Clean Product Baseline

## Product intent

Aurora is developed as one integrated low-cost 7.1.4-class soundbar/rear-speaker product, not as a collection of competing hardware demos. The experience target is the functional class of a premium Samsung Q995-series system: stable TV audio ingest, immersive 7.1.4 playback, strong bass integration, automatic setup, synchronized wireless rear speakers, low lip-sync error, robust source switching, consumer-style control, and long-duration reliability.

This target is an engineering benchmark. Aurora does not claim Samsung equivalence, Dolby Atmos compatibility, trademark compatibility, or acoustic parity without the required legal and measured physical evidence.

## Single product truth

The canonical development branch remains `main-v2`. During consolidation, PR #85 (`product/q995-clean-baseline`) is the only product-consolidation track.

The product has one primary hardware lane:

`TV/eARC -> realtime-MCU capture/I/O boundary -> Galaxy S6 appliance -> immersive decoder/renderer boundary -> Aurora DSP/output control -> realtime-MCU -> DAC/amplification -> soundbar + sub + rear nodes`

The Galaxy S6 lane remains primary because its appliance, live-ingest, source-manager, transport, build and MCU-portable foundations are already integrated on `main-v2`. It must be replaced only if physical CPU, thermal, latency, xrun, USB or decode/render measurements demonstrate that it cannot meet the acceptance limits.

The i.MX93 R0/R1/R2 branches are engineering references and fallback hardware research. Their strongest general ideas are adopted as product invariants where platform-independent: direct bounded audio paths, explicit clock matching, fail-closed source ownership, bounded queues, minimal process crossings, deterministic recovery and a single appliance owner. They are not a second product roadmap.

## Canonical internal audio contract

The immersive product path uses:

- 48 kHz processing;
- 7.1.4 logical output as 12 full-band/LFE channels at the immersive-render boundary;
- explicit channel ordering and layout hash on transport boundaries;
- bounded fixed-period realtime transport on the S6/realtime-MCU path;
- no hidden channel remapping in callbacks;
- no allocation, blocking I/O, filesystem access, process spawning or formatted logging in callback-reachable steady-state code.

A 7.1.4 PCM transport contract is not evidence that 3D rendering, height perception or acoustic output is correct. Each layer has a separate acceptance gate.

## Rendering architecture

Aurora keeps three renderer classes distinct:

1. `GeometricBinaural`: lightweight stereo ITD/ILD reference only. It is not HRTF.
2. `aurora-renderer-vbap`: accepted horizontal 2D loudspeaker renderer. It is not height capable.
3. `aurora-renderer-vbap3d`: the required height-capable Aurora loudspeaker renderer for Aurora-native spatial scenes. It must use genuine 3D speaker geometry and must be validated with front, side, rear and overhead probes before realtime promotion.

External immersive decoding/rendering such as the staged Harletty + Omniphony path remains isolated behind Aurora-owned transport and post-processing boundaries. Aurora does not reproduce proprietary Dolby codec behavior.

## DSP required for the product

A Q995-class Aurora release requires one deterministic output DSP graph after immersive rendering/decoding and before physical output:

1. channel-map validation and headroom reservation;
2. per-channel polarity and gain trim;
3. calibration-derived delay alignment;
4. bass management and LFE routing with configurable crossover policy;
5. per-channel PEQ and optional FIR correction;
6. speaker/room target shaping;
7. limiter and clipping protection;
8. final format conversion and transport packing.

The existing fractional-delay processor is only one part of this graph. Missing DSP blocks stay explicitly `implementation_required` in `config/product-capabilities.json` until implemented and accepted.

## Automatic setup target

The product calibration workflow must estimate, with measurable confidence:

- speaker presence and channel identity;
- polarity;
- acoustic distance / time of flight;
- relative level;
- usable frequency response;
- crossover-relevant low-frequency behavior;
- room decay/RT60 estimate where measurement quality allows;
- rear/height synchronization offset;
- final delay, gain and EQ parameters.

A phone may be used as a setup microphone/controller, but final values must be traceable to captured measurements and not inferred from device geometry alone.

## Wireless rear target

Rear nodes are part of the theater product, not generic multiroom. Theater rear transport requires:

- bounded packet format and sequence/sample-time semantics;
- independent clock estimation and correction;
- deterministic startup buffering;
- bounded loss/reorder behavior;
- measured inter-speaker skew;
- mute/ramp behavior on discontinuity;
- fail-closed recovery after coordinator loss;
- no claim of synchronization based only on same-process queues or simulation.

Multiroom remains a later operating mode sharing transport primitives but using different latency and recovery thresholds.

## Source and appliance model

There is exactly one active final audio owner. Source adapters may warm up, but only the granted source may reach the final DSP/output path. Source handoff must remain explicit, acknowledged and fail-closed.

The mature product should converge toward one appliance owner for audio orchestration. Helper processes are allowed only where isolation or third-party boundaries justify them; unnecessary process crossings in the realtime data path are rejected.

## Product acceptance levels

Every capability uses one of these evidence states:

- `implementation_required` / `placeholder`: no product implementation accepted;
- `experimental_host_validated`: executable software exists but capability limits remain experimental;
- `host_validated`: deterministic software/CI evidence exists;
- `host_validated_physical_open`: software boundary is accepted but physical hardware evidence is missing;
- `physical_validated`: required measurements on target hardware pass;
- `production_ready`: physical acceptance, failure recovery, soak, documentation and legal boundaries all pass.

No README, UI, release, installer or agent may promote a lower evidence state to a higher one.

## Q995-class release gates

The product may be described as Q995-class only after all of the following have measured physical evidence on the selected hardware:

- stable real TV/eARC ingest and source switching;
- lawful immersive input path producing validated 7.1.4 behavior;
- front/side/rear/height channel mapping verified acoustically;
- bass-management and sub integration accepted;
- calibration workflow accepted in multiple room geometries;
- wireless rear synchronization within the defined skew limit;
- lip-sync within the product limit;
- no sustained xruns during immersive playback;
- acceptable S6/replacement-host thermal behavior;
- limiter/headroom behavior verified at high output;
- restart, source-loss, USB/network loss and recovery behavior accepted;
- extended thermal/reliability soak passes.

Until those gates pass, the correct description is: **Aurora targets a Q995-class 7.1.4 experience; software and hardware capabilities are reported individually by evidence state.**
