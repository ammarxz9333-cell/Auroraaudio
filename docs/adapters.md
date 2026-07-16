# Third-Party Adapter Layer

Milestone 0D creates adapter crates only. Aurora does not copy third-party source into the repository and does not link third-party decoder, renderer, or DSP code by default.

## Adapter Policy

- Adapters implement Aurora-owned traits.
- Third-party tools should run out-of-process where practical.
- Adapter-specific Cargo features are disabled by default.
- `aurora-core` must compile and test without any adapter crate.
- Commercial use requires a separate license and patent review.

## CamillaDSP

- Crate: `aurora-dsp-camilladsp`
- Aurora trait: `aurora_dsp_api::DspEngine`
- Cargo feature: `camilladsp-process`
- Integration style: generated CamillaDSP YAML plus offline file processing through an external process.
- Maturity: functional offline adapter.
- Production readiness: not production-ready.
- Commercial risk: medium; CamillaDSP licensing and deployment obligations must be reviewed before redistribution.
- Source policy: do not fork or vendor CamillaDSP source.
- Executable discovery order: explicit `--camilladsp-path`, `AURORA_CAMILLADSP_PATH`, then `PATH`.
- Supported initial controls: channel count, sample rate, WAV input/output, per-channel gain, mute, polarity inversion, delay, high-pass, low-pass, and parametric EQ.
- Process safety: invoked without shell concatenation, stdout/stderr captured, timeout enforced, output WAV validated.
- Local validation: tested against CamillaDSP 4.1.3 using `WavFile` capture, `File` playback with `wav_header: true`, sample-based delays, and per-channel filter pipelines.

## IAMF / libiamf

- Crate: `aurora-decoder-iamf`
- Aurora trait: `aurora_decoder_api::Decoder`
- Cargo feature: `libiamf-process`
- Integration style: preferred open immersive-audio decoder through an external libiamf-compatible process.
- Maturity: preferred open adapter candidate.
- Production readiness: not production-ready.
- Commercial risk: medium; codec, patent, and distribution posture must be reviewed.
- Source policy: do not copy libiamf source into Aurora.

## truehdd

- Crate: `aurora-decoder-truehdd`
- Aurora trait: `aurora_decoder_api::Decoder`
- Cargo feature: `truehdd-process`
- Integration style: experimental offline-only external process if ever enabled.
- Maturity: experimental/offline-only.
- Production readiness: not production-ready.
- Commercial risk: high; must not be used for product behavior without legal review.
- Source policy: do not copy truehdd source into Aurora.

## Cavern

- Crate: `aurora-renderer-cavern`
- Aurora trait: `aurora_renderer_api::Renderer`
- Cargo feature: `cavern-process`
- Integration style: disabled-by-default external/process adapter pending license review.
- Maturity: blocked pending license review.
- Production readiness: not production-ready.
- Commercial risk: high until license and redistribution questions are resolved.
- Source policy: do not copy Cavern source into Aurora.
