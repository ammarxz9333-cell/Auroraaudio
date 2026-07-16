# Aurora Spatial Audio Platform

Aurora is a Phase 0 prototype for an open, modular spatial-audio processing platform.

This checkpoint intentionally excludes HDMI/eARC capture, Dolby/DTS decoding, wireless speakers, hardware amplification, and visualizer work. The current implementation focuses on core data structures, a renderer trait, a deterministic inverse-distance renderer, offline WAV rendering, tests, and CLI commands for gain inspection and file rendering.

## Repository Status

`main-v2` is the canonical development branch. The earlier unrelated GitHub
history remains archived without merging or rewriting. See
[`docs/repository-history.md`](docs/repository-history.md) for the branch and
acceptance boundaries.

## Build

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo bench --workspace
```

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

Optimization Sprint 2A established the reviewed foundation: Aurora-owned
stable-descriptor/timing/fault contracts, a hardware-independent bounded duplex
bridge, synthetic clock-drift tests, and a captured-signal latency estimator are
implemented. See `docs/backend-timing.md`,
`docs/duplex-audio.md`, `docs/duplex-transport.md`,
`docs/drift-compensation.md`, and `docs/latency-measurement.md`.

Optimization Sprint 2B adds separate live input/output streams, Rubato-backed
adaptive asynchronous resampling, a bounded PI drift controller, explicit duplex
device states, physical loopback measurement, and bounded soak reporting. See
`docs/live-duplex.md`, `docs/asynchronous-resampling.md`,
`docs/drift-controller.md`, and `docs/physical-latency-validation.md`.

```text
cargo run -p aurora-cli --all-features -- duplex --input-device <selector> --output-device <selector> --sample-rate 48000 --block-size 256 --channels 2
cargo run -p aurora-cli --all-features -- measure-latency --input-device <selector> --output-device <selector> --sample-rate 48000 --block-size 256 --duration-seconds 10
cargo run -p aurora-cli --all-features -- duplex-soak --input-device <selector> --output-device <selector> --duration-minutes 60 --report output/duplex-soak.json
```

Simulation Sprint 1 adds deterministic virtual devices and accelerated duplex,
loopback, routing, and fault validation without CPAL or physical hardware:

```text
cargo run -p aurora-cli --all-features -- simulate-duplex --profile usb-7-1 --duration-hours 24 --seed 12345 --report output/simulation/usb-7-1-24h.json
cargo run -p aurora-cli --all-features -- simulate-latency --profile usb-5-1 --loopback-delay-frames 777 --jitter-frames 3 --noise-db -60 --seed 42
cargo run -p aurora-cli --all-features -- simulate-output-validation --profile usb-7-1 --layout 7.1 --report output/simulation/validation-7-1.json
```

Simulation reports use explicit simulated-truth labels. They are not measured
hardware latency or evidence about a physical driver. See
`docs/simulation-backend.md` and `docs/simulation-validation.md`.

## Performance Baseline

Benchmarks always use Cargo's optimized bench profile:

```powershell
cargo bench --workspace
cargo bench -p aurora-realtime-engine --bench baseline
cargo bench -p aurora-realtime-engine --bench performance
```

The `baseline` target prints median and p95 time per block, percentage of the
48 kHz block budget, and an estimated sustainable channel count for 64, 128, 256,
and 512 frames with 2, 6, 8, and 12 outputs. Criterion stores detailed reports in
`target/criterion`. See `docs/profiling.md` for Windows profiling guidance.

## Adapter Layer

Milestone 0D includes adapter crates for CamillaDSP, IAMF/libiamf, truehdd, and Cavern. These crates define Aurora-owned boundaries only and do not vendor third-party source. See `docs/adapters.md`.

Milestone 0E makes the CamillaDSP adapter functional for offline processing when an external `camilladsp` executable is installed. Discovery checks `--camilladsp-path`, `AURORA_CAMILLADSP_PATH`, then `PATH`.
