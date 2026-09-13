# Aurora canonical handoff

This file is the compact source of truth for the next agent working on Aurora. Always fetch the current branch/PR head before editing; do not assume a SHA in notes is still the latest.

## Repository state

- Repository: `ammarxz9333-cell/Auroraaudio`
- Default branch: `main-v2`
- Active software-gap branch: `gap-closure-software-p0`
- Draft PR: `#156` — `Close P0 software resilience gaps without selecting hardware`
- Commit `7d9bd815d916a31356f40ca4b6ba1dc5b373aa11` completed a 9/9 green PR matrix after promoting the narrow OAR stereo object differential.
- The OAR stereo object differential first passed on `9f7e19a73790527ff7ed2da2dd2dd48df5f34027`, OAR Reference CI run `34773581397`.
- The first real IAMF rendered-channel reference lane passed on commit `6bd62667f429588b010f372d1aec51ea88340933`, IAMF Rendered PCM Reference CI run `34775520753`.
- Do not merge or move `main-v2` without an explicit user request.

## Architecture rule: hardware is not selected

Aurora is currently a hardware-agnostic software architecture. No host CPU, SBC, MCU, eARC bridge, USB/TDM interface, DAC, amplifier, wireless transport chipset, speaker product, or OS image is frozen.

Historical Pi/CM5, Intel N100, STM32H753, Lindy/SiI9437, MCHStreamer, PCM3168A/CS42448 and similar references are evidence paths or candidates only. Do not convert them into architecture requirements.

Current media direction is capability driven:

`encoded/source input -> StreamingDecoder/Decoder API -> source PCM + explicit object/channel semantics -> renderer/mixer -> DSP -> generic realtime audio I/O`

Physical eARC, output transport, DAC/amplifier and acoustic validation remain separate future acceptance layers.

## Decoder / immersive state

Do not say Aurora has no decoder. A real JOC software path exists and is validated as software-reference evidence.

- `aurora-decoder-api` exposes a hardware-neutral streaming decoder contract.
- `StreamingDecodedFrame` carries decoded PCM, semantic channel kinds and explicit object-id -> PCM-channel bindings.
- `LiveImmersiveRuntime` validates transport/type/PCM/object semantics, primes before releasing audio, preserves bed routing, renders object gains, mixes object PCM and fails closed on malformed input.
- `PanicIsolatedStreamingDecoder` contains in-process decoder panics on the media/control side, latches the failure and requires explicit recovery.
- Pinned Harletty JOC has passed through Aurora's live decoder/runtime contract to non-zero finite speaker PCM.
- Plain E-AC-3 is a fail-closed negative control for native-object claims.
- This does not prove protected-service compatibility, physical eARC, Dolby certification or final acoustic quality.

## Completed P0 realtime resilience

The current PR contains and has software/virtual evidence for:

- fixed-storage frame-count PPM estimator with 10 s windows, median-of-3 filtering, trust gating and discontinuity reset;
- estimator feed-forward into `AdaptiveDuplexConsumer -> DriftController -> RubatoAsrc`;
- bounded correction slew and +250/-250 ppm long-run virtual evidence;
- jitter rejection, continuous clock-step convergence and discontinuity/reacquisition profiles;
- out-of-range clock relationships fail closed;
- production adaptive hard-fault latch: controller/resampler/capacity failures publish Fatal health and remain muted across later callbacks until explicit control-plane bridge rebuild;
- bounded reconnect state machine with 250/500/1000/2000/4000 ms backoff, exhaustion after five attempts, stable-run reset and flapping-device history retention;
- bounded live IEC61937 ingress diagnostics and threshold-driven verdicts;
- paced wall-clock and integrated faulted soak evidence;
- panic isolation and explicit live-runtime recovery/re-priming.

Do not reintroduce sparse sample-slip correction as a solution for hundreds of ppm. At 48 kHz, one sample per 10 s is only about 2.083 ppm. ASRC is the main correction mechanism; unsupported mismatch must fail closed.

## OAR differential reference lane

Pinned external reference:

- AOMedia OAR v1.0.0
- commit `5601d50c05a5e71cac7e80babeff7dd2a53b2060`
- external-reference-only; respect upstream license and PATENTS terms.

The implemented differential is intentionally narrow: stereo 2D point-object panning semantics, not IAMF playback.

Evidence files:

- `config/oar-evaluation-v1.json`
- `validation/open-immersive/oar_differential_probe.c`
- `crates/aurora-renderer-vbap/examples/oar_differential_probe.rs`
- `validation/open-immersive/oar_differential_evidence.py`
- `validation/open-immersive/test-oar-differential.sh`
- `.github/workflows/oar-reference-ci.yml`

The probes compare identical point-object azimuth cases `-60, -30, -15, 0, +15, +30, +60` degrees at 48 kHz / 256 frames using normalized left/right output power. OAR positive azimuth is listener-left; Aurora coordinates are mapped accordingly (`+X` right, `+Y` front).

Reference run `34773581397` passed with:

- maximum left-power-share absolute delta: about `3.6e-8`;
- left-power-share RMSE: about `1.9243e-8`;
- center: exactly `0.5 / 0.5` in both renderers;
- mirror-symmetry max error: `0` in both renderers;
- no monotonicity or direction violations.

Coverage therefore distinguishes `oar-vbap-object-differential` as covered software-reference evidence while broader IAMF/OAR rendering remains planned.

Do not infer IAMF parse/decode support, 7.1.4 equivalence, elevation rendering, HOA, binaural equivalence, Atmos equivalence or certification from the stereo differential.

## IAMF rendered-channel reference lane

Aurora now also has a deliberately separate IAMF complete-file reference path. It is a rendered-channel validation lane, not an IAMF object-scene integration.

Pinned references:

- `AOMediaCodec/libiamf` commit `e55e1832a608affe602de2ee39929bd7759a75ab`;
- the libiamf OAR submodule commit `3d1d23b807543f993a1d0cf0a9839c7f0746d94b`;
- official `AOMediaCodec/iamf-tools` fixture `iamf/cli/testdata/iamf/noise_1024samp_5p1_opus.iamf` at commit `d13b8dd52211f0c77cde4a7a8fbc8ca84ae75b09`.

`IamfRenderedPcmReferenceDecoder`, behind the `libiamf-process` feature, invokes pinned `iamfdec` without a shell and imports only the rendered PCM as `DecoderOutputSemantics::ChannelPcm`. It accepts one complete standalone IAMF bitstream per offline `decode_chunk` call, currently fixes the reference output to stereo / 48 kHz / signed PCM32 from `iamfdec`, converts that deterministically to Aurora planar F32 and always emits an empty object list.

IAMF Rendered PCM Reference CI run `34775520753` passed with:

- real pinned libiamf build and `iamfdec` execution;
- official IAMF fixture decode;
- `1024` output frames / `2048` stereo samples at `48 kHz`;
- peak absolute sample about `0.8912509`, RMS about `0.35538234`;
- `0` sample-bit mismatches between direct libiamf PCM and Aurora-imported PCM after the declared PCM32-to-F32 normalization;
- `0` fabricated objects.

Coverage may therefore mark `iamf-rendered-channel-pcm-reference` as covered software-reference evidence. Keep `iamf-oar-open-rendering` planned: the current process adapter does not expose IAMF source object/audio-element metadata or complete object-to-PCM bindings into Aurora's renderer. It does not prove live IAMF streaming, 7.1.4/elevation/HOA rendering inside Aurora, binaural behavior, physical output, protected-service compatibility or certification.

Evidence files:

- `config/iamf-reference-v1.json`
- `crates/aurora-decoder-iamf/src/lib.rs`
- `crates/aurora-decoder-iamf/examples/iamf_rendered_pcm_probe.rs`
- `validation/open-immersive/iamf_reference_evidence.py`
- `.github/workflows/iamf-reference-ci.yml`

## Simulation / evidence truth boundaries

`config/simulation-coverage-v1.json` is authoritative for promoted versus planned capabilities. Keep gaps explicit.

Software/virtual evidence never proves:

- physical eARC discovery/capture or electrical timing;
- real USB/TDM/DAC clock-domain behavior;
- physical hotplug/reconnect behavior;
- room/acoustic performance;
- legitimate Netflix/Disney+/Prime protected-service Atmos compatibility;
- Dolby certification/conformance.

Protected-service validation must be performed lawfully on authorized paths. Do not add DRM circumvention, license-check bypasses or source tricks intended to obtain capabilities a protected service or codec stack does not authorize.

## Current next software work

1. Keep the libiamf rendered-ChannelPcm reference lane and tightened OAR stereo point-object differential green.
2. Investigate a real IAMF source-element/object metadata boundary that can provide complete object/audio-element semantics and object-to-PCM bindings before promoting `iamf-oar-open-rendering`.
3. Add OAR multichannel/elevation differential cases separately; add HOA only when Aurora has a real scene-based lane to compare.
4. Build the ADM/BS.2127 EAR/libear differential lane.
5. Review authored/sample-accurate object metadata interpolation/timeline semantics in Aurora's live mixer; do not overclaim full temporal Atmos fidelity from the current frame-level validation adapter.
6. Continue other explicit planned software gaps only with executable evidence and fail-closed negative controls.
7. Do not select hardware yet.
8. Physical issue #143 and legitimate protected-service validation come only after an explicit future hardware/integration decision.

## CI discipline

- Use the latest branch head only.
- Keep Rust MSRV at 1.78 where the workspace requires it.
- `cargo fmt --all --check`, Clippy with warnings denied and workspace tests must stay green on Linux/Windows as configured.
- A capability is not `covered` merely because code exists. It must have executable evidence and a truth boundary.
- Negative controls must fail closed; avoid unconditional `pass` verdicts.
- Do not claim a final-head green matrix until every relevant workflow for that exact head has completed successfully.
