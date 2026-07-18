# Aurora Spatial Audio Platform

Aurora is an open, modular, hardware-independent spatial-audio processing platform written in Rust.

The project is now in **active product implementation**. The immediate work is to stabilize the landed geometric binaural prototype, establish a common evaluation and capability-reporting layer, implement offline 3D loudspeaker rendering, then build true HRTF, open IAMF input, synchronized IP transport, Linux receiver nodes, and multiroom behavior.

Aurora does not currently claim Dolby Atmos compatibility, true HRTF capability, height-capable loudspeaker rendering, production IAMF decoding, synchronized wireless speakers, or physical multiroom validation unless the corresponding acceptance evidence exists.

## Geometric binaural baseline

`GeometricBinaural` is Aurora's lightweight two-channel headphone baseline. It
uses geometric interaural time difference (ITD), geometric interaural level
difference (ILD), and simple distance attenuation. It is **not an HRTF
renderer**: it uses no HRIR data, convolution, pinna cues, or elevation cues.

The canonical CLI value is `--renderer-mode geometric-binaural`. The former
`--renderer-mode binaural` spelling remains accepted as a deprecated alias for
command compatibility. The Rust enum variant is intentionally renamed from
`BasicRendererMode::Binaural` to
`BasicRendererMode::GeometricBinaural`; downstream Rust callers must update.

```powershell
cargo run -p aurora-cli -- render `
  --scene fixtures\scenes\geometric_binaural_circle.json `
  --input fixtures\audio\mono_sweep.wav `
  --output output\geometric_binaural_circle.wav `
  --renderer-mode geometric-binaural
```

Geometric delays are recalculated once per block and applied directly to the
existing fractional delay line. Checkpoint A verifies that the geometric delay
trajectory is continuous, but it does not add crossfading or interpolation
between block updates and does not claim click-free moving-source output. The
current dynamic delay capacity remains the existing explicit `1024` samples;
deriving it from future scene/runtime bounds belongs to Checkpoint B.

## Repository status

`main-v2` is the canonical development branch. The earlier unrelated GitHub history remains archived without merging or rewriting.

Start here:

- [Current execution state](PROJECT_EXECUTION_STATE.md)
- [Authoritative product execution roadmap](docs/roadmaps/immersive-wireless-audio-execution-roadmap.md)
- [Master project reference](AURORA_MASTER_REFERENCE.md)
- [Architecture](docs/architecture.md)
- [Repository history](docs/repository-history.md)

Historical governance, ADRs, and acceptance records remain available, but they do not override the active product execution sequence.

## Active sequence

1. issue `#43`, Checkpoint A — stabilize geometric binaural;
2. issue `#44` — unified evaluation and artifact runner;
3. issue `#45` — capability registry and CLI;
4. issue `#38` — offline 3D loudspeaker rendering;
5. issue `#46` — SOFA/HRIR backend;
6. true offline and realtime Aurora HRTF;
7. operational IAMF integration;
8. CamillaDSP runtime hardening;
9. deterministic network simulation and packet transport;
10. Linux receiver, optional PipeWire backend, and multiroom behavior;
11. external Steam Audio and Snapcast comparisons;
12. minimum physical validation and measurement-driven hardware selection.

## Build

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo bench --workspace
```

## Continuous integration

Linux stable is the main validation environment. Windows stable verifies cross-platform compilation and software-only tests, and a Linux job checks the declared Rust 1.78 MSRV explicitly. Stable jobs run formatting, all-target and all-feature Clippy, workspace tests, and strict public documentation checks; the MSRV job performs locked all-target/all-feature checks and tests.

All CI results are software evidence only. Ignored hardware tests remain hardware-gated, and no CI result is a physical measurement. Future renderer PRs must also publish deterministic WAV and machine-readable evaluation artifacts through issue `#44`.

## Run

```powershell
cargo run -p aurora-cli -- gains --layout stereo --steps 12
cargo run -p aurora-cli -- render --scene fixtures/scenes/circle.json --input fixtures/audio/mono_sweep.wav --output output/scene_5_1.wav
cargo run -p aurora-cli -- render --scene fixtures/scenes/asymmetric_5_1.json --input fixtures/audio/mono_sweep.wav --output output/asymmetric_5_1.wav
cargo run -p aurora-cli -- doctor
cargo run -p aurora-cli --all-features -- devices
cargo run -p aurora-cli --all-features -- realtime --output-device 0 --scene fixtures/scenes/asymmetric_5_1.json --sample-rate 48000 --block-size 256 --apply-geometric-delay --test-signal rotating-sine
cargo run -p aurora-cli --all-features -- identify-speakers --output-device 0 --layout five-one
cargo run -p aurora-cli --all-features -- process --engine camilladsp --scene fixtures/scenes/asymmetric_5_1.json --input output/asymmetric_5_1.wav --output output/asymmetric_5_1_processed.wav --config fixtures/dsp/basic_5_1.json --apply-geometric-delay
```

The `gains` command prints one row per simulated source position with per-speaker gains. The `render` command reads a mono WAV, renders it through the scene speaker layout, and writes a multichannel WAV offline.

The `devices`, `realtime`, and `identify-speakers` commands exercise the local real-time path. `identify-speakers` requires explicit confirmation before playback.

## Duplex and timing foundations

Optimization Sprint 2A established Aurora-owned stable-descriptor, timing, and fault contracts; a hardware-independent bounded duplex bridge; synthetic clock-drift tests; and a captured-signal latency estimator.

Optimization Sprint 2B adds separate live input and output streams, Rubato-backed adaptive asynchronous resampling, a bounded PI drift controller, explicit duplex device states, physical loopback measurement, and bounded soak reporting.

```text
cargo run -p aurora-cli --all-features -- duplex --input-device <selector> --output-device <selector> --sample-rate 48000 --block-size 256 --channels 2
cargo run -p aurora-cli --all-features -- measure-latency --input-device <selector> --output-device <selector> --sample-rate 48000 --block-size 256 --duration-seconds 10
cargo run -p aurora-cli --all-features -- duplex-soak --input-device <selector> --output-device <selector> --duration-minutes 60 --report output/duplex-soak.json
```

## Simulation foundations

Deterministic virtual devices support accelerated duplex, loopback, routing, and fault validation without CPAL or physical hardware:

```text
cargo run -p aurora-cli --all-features -- simulate-duplex --profile usb-7-1 --duration-hours 24 --seed 12345 --report output/simulation/usb-7-1-24h.json
cargo run -p aurora-cli --all-features -- simulate-latency --profile usb-5-1 --loopback-delay-frames 777 --jitter-frames 3 --noise-db -60 --seed 42
cargo run -p aurora-cli --all-features -- simulate-output-validation --profile usb-7-1 --layout 7.1 --report output/simulation/validation-7-1.json
```

Simulation reports use explicit simulated-truth labels. They are not measured hardware latency or evidence about a physical driver.

## Performance baseline

Benchmarks always use Cargo's optimized bench profile:

```powershell
cargo bench --workspace
cargo bench -p aurora-realtime-engine --bench baseline
cargo bench -p aurora-realtime-engine --bench performance
```

The `baseline` target prints median and p95 time per block, percentage of the 48 kHz block budget, and an estimated sustainable channel count for 64, 128, 256, and 512 frames with 2, 6, 8, and 12 outputs. Criterion stores detailed reports in `target/criterion`.

## Third-party integration policy

Aurora keeps scene representation, rendering policy, evaluation, timing, transport, receiver behavior, and product orchestration under Aurora-owned interfaces.

Preferred optional integrations:

- `libmysofa` for user-supplied or redistributable SOFA/HRIR data;
- `iamf-tools` or `libiamf` through an isolated process for open immersive input;
- CamillaDSP as an external DSP backend;
- PipeWire as a future optional advanced Linux backend;
- Steam Audio as an external HRTF and room-simulation comparison;
- Snapcast as an external multiroom synchronization comparison.

Cavern, truehdd, and Resonance Audio are not active first-release dependencies. See [Third-party adapters](docs/adapters.md) and [Third-party licenses](THIRD_PARTY_LICENSES.md).
