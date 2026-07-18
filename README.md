# Aurora Spatial Audio Platform

Aurora is an open, modular, hardware-independent spatial-audio processing platform written in Rust.

The project is in **active product implementation**. The immediate work is to stabilize the landed geometric binaural prototype, establish common evaluation, performance, memory, ingestion, and capability foundations, implement offline 3D loudspeaker rendering, then build true HRTF, open IAMF input, synchronized IP transport, Linux receiver nodes, and multiroom behavior.

Aurora does not currently claim Dolby Atmos compatibility, true HRTF capability, height-capable loudspeaker rendering, production IAMF decoding, synchronized wireless speakers, or physical multiroom validation unless the corresponding acceptance evidence exists.

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
2. issue `#44` — unified evaluation, benchmark, artifact, and memory-evidence runner;
3. issue `#48` — Symphonia-based WAV/FLAC/Ogg ingestion;
4. issue `#45` — capability registry and CLI;
5. issue `#38` — offline 3D loudspeaker rendering;
6. issue `#46` — SOFA/HRIR backend and narrow FFI boundary;
7. true offline and realtime Aurora HRTF;
8. operational out-of-process IAMF integration;
9. CamillaDSP runtime hardening;
10. deterministic network simulation and Tokio-based packet transport outside realtime callbacks;
11. Linux receiver, optional PipeWire backend, and multiroom behavior;
12. external Steam Audio and Snapcast comparisons;
13. minimum physical validation and measurement-driven hardware selection.

Issues `#48` and `#45` may proceed in parallel after issue `#44`, but each requires a separate PR.

## Build

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo bench --workspace
```

## Continuous integration

Linux stable is the main validation environment. Windows stable verifies cross-platform compilation and software-only tests, and a Linux job checks the declared Rust 1.78 MSRV explicitly. Stable jobs run formatting, all-target and all-feature Clippy, workspace tests, and strict public documentation checks; the MSRV job performs locked all-target/all-feature checks and tests.

All CI results are software evidence only. Ignored hardware tests remain hardware-gated, and no CI result is a physical measurement. Future renderer and transport PRs must publish deterministic artifacts and machine-readable performance evidence through issue `#44`.

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

The `gains` command prints one row per simulated source position with per-speaker gains. The `render` command currently reads a mono WAV and writes a multichannel WAV offline. Issue `#48` will add Aurora-owned multi-format ingestion without placing decoding in the realtime callback.

The `devices`, `realtime`, and `identify-speakers` commands exercise the local realtime path. `identify-speakers` requires explicit confirmation before playback.

## Rust foundation policy

- **Symphonia:** offline and streaming-file ingestion behind Aurora-owned PCM types; WAV, FLAC, and Ogg/Vorbis first.
- **CPAL:** retained as the cross-platform realtime audio backend.
- **Tokio:** control plane, discovery, socket orchestration, reconnect logic, and background services only; never required by renderer, DSP, or device callbacks.
- **mio:** considered only if measured Tokio benchmarks fail requirements.
- **Criterion:** benchmark and regression-evidence foundation.
- **Bytehound or equivalent:** external Linux memory-profiling workflow.
- **Bencher:** optional after Aurora's benchmark artifact schema stabilizes.
- **tracing:** structured telemetry, with heavy formatting and output outside realtime paths.

Rodio and PortAudio are not planned as product architecture because Aurora requires direct device timing, channel-map, block, and callback control.

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

The `baseline` target prints median and p95 time per block, percentage of the 48 kHz block budget, and an estimated sustainable channel count. Issue `#44` expands this into machine-readable p50/p95/p99 reports, explicit regression thresholds, CI artifacts, and a documented memory-profiling procedure.

## Third-party and FFI policy

Aurora keeps PCM contracts, scene representation, rendering policy, evaluation, timing, transport, receiver behavior, and product orchestration under Aurora-owned interfaces.

Preferred optional integrations:

- `libmysofa` for user-supplied or redistributable SOFA/HRIR data through narrow reviewed FFI;
- `iamf-tools` or `libiamf` through an isolated process for open immersive input;
- CamillaDSP as an external DSP backend;
- PipeWire as a future optional advanced Linux backend;
- Steam Audio as an external HRTF and room-simulation comparison;
- Snapcast as an external multiroom synchronization comparison.

Unsafe/native code must remain inside small dedicated adapter crates. Raw pointers and native structs must not cross Aurora-owned public APIs.

Cavern, truehdd, and Resonance Audio are not active first-release dependencies. See [Third-party adapters](docs/adapters.md) and [Third-party licenses](THIRD_PARTY_LICENSES.md).