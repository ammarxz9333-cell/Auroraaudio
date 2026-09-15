# Phase 11 — 7.1.4 binaural semantic differential

This lane compares Aurora's deterministic geometric binaural model with the exact-pinned Google Open Binaural Renderer (OBR) for channel-isolated 7.1.4 PCM.

## Scope

The Aurora side is `aurora-renderer-basic` in `GeometricBinaural` mode. It exposes geometric interaural time difference (ITD), geometric interaural level difference (ILD), and relative per-ear distance weighting. It is intentionally not an HRTF/HRIR renderer.

The external oracle is Google OBR at commit `478dc7c752d5eccae534635139ff0253eee3a14a`, invoked only as an external executable built from `//obr/cli:obr_cli`.

## 7.1.4 channel-order boundary

OBR consumes interleaved PCM channels sequentially for the declared 7.1.4 input type. The order used by both this validation lane and the Aurora probe is:

`FL, FR, FC, LFE, SL, SR, SBL, SBR, TFL, TFR, TRL, TRR`

This is an explicit semantic boundary. No WAVE_FORMAT_EXTENSIBLE speaker-mask permutation is applied here. That differs from file/device boundaries where physical WAVE channel-mask ordering may require a role-aware remap.

A negative CI control swaps `SL` and `SBL` and requires the analyzer to fail closed.

## Deterministic fixtures

For each of the 12 channels, the validator creates a separate 16-bit / 48 kHz / 8192-frame PCM fixture with a single impulse in only that channel. The frame count is an exact multiple of OBR's 256-frame processing block. OBR renders each fixture using the `Direct` binaural filter profile.

The 11 spatial channels are checked for:

- stereo output format and exact frame accounting;
- finite, non-zero output energy;
- expected left/right/center power-bias semantics;
- left/right mirror-pair consistency;
- agreement with the directional semantic class produced by Aurora's geometric binaural probe.

LFE is executed to verify the path and output integrity, but this lane deliberately makes no spatial-direction claim for LFE.

## Why this is not raw-PCM comparison

Aurora's current geometric binaural model and OBR do not implement the same transfer function. Raw sample correlation or waveform equality would therefore be a false acceptance criterion. This lane compares invariants that both implementations should preserve: left/right directional bias, mirror symmetry, finite output, and deterministic frame accounting.

## Truth boundary

A green result is software/reference evidence only. It does **not** prove:

- HRTF or HRIR parity;
- personalized HRTF quality;
- front/back discrimination;
- elevation discrimination;
- head-tracker or head-rotation behavior;
- perceptual quality;
- physical output latency;
- headphone or loudspeaker behavior;
- Dolby, DTS, HDMI, or other certification/conformance.

Those remain separate Phase 11 or physical-evidence gates.
