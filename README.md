# Aurora

Aurora is an open, modular, **hardware-agnostic immersive-audio software stack** written primarily in Rust.

Aurora is not tied to a phone, SBC, MCU, soundbar, DAC, amplifier, or vendor platform. The core project owns audio/scene models, rendering, DSP, realtime scheduling, simulation, validation, and generic I/O boundaries. Physical transports and devices belong behind optional adapters and must never become prerequisites for the core engine.

## Project direction

Aurora is software-first. The active goals are:

- open scene and channel/object processing boundaries;
- offline and realtime multichannel rendering, including 7.1.4-class layouts;
- pluggable decoder/renderer/DSP backends;
- deterministic simulation and repeatable validation;
- generic local audio I/O through portable APIs;
- explicit capability reporting so documentation cannot claim more than the code proves.

Aurora does **not** require Galaxy S6, Android, N100, Raspberry Pi, STM32, a specific eARC receiver, or any other hardware target. Hardware integration may be added later as optional adapters maintained outside the software core.

Aurora also does not claim Dolby certification, DRM circumvention, or production compatibility with proprietary streaming services.

## Capability truth registry

The canonical capability state is a versioned typed registry owned by `aurora-core`. This compact table is generated from that registry and is checked in CI.

<!-- AURORA_CAPABILITIES_BEGIN -->

| Capability | Implementation | Offline | Realtime | Verification | Tested layouts | Production-ready |
| --- | --- | --- | --- | --- | --- | --- |
| `geometric-binaural` — Geometric binaural renderer | experimental | functional | experimental | ci-artifact | stereo | no |
| `iamf` — IAMF decoder adapter | adapter-placeholder | unsupported | unsupported | none | none | no |
| `camilladsp` — CamillaDSP external adapter | functional | functional | unsupported | software-tested | 5.1 | no |
| `cavern` — Cavern renderer adapter | inactive-research | unsupported | unsupported | none | none | no |
| `truehdd` — truehdd decoder adapter | inactive-research | unsupported | unsupported | none | none | no |
| `loudspeaker-3d` — 3D loudspeaker renderer | not-implemented | unsupported | unsupported | none | none | no |

<!-- AURORA_CAPABILITIES_END -->

## Architecture

The repository is organized around reusable software components:

- `aurora-core`: channel/layout/capability foundations;
- `aurora-scene`: speaker and object scene representation;
- `aurora-renderer-*`: renderer API and implementations;
- `aurora-dsp-*`: DSP API and implementations;
- `aurora-realtime-*`: realtime audio, timing, drift, ASRC, and transport-independent scheduling;
- `aurora-audio-io`: generic audio I/O abstractions;
- `aurora-config`, `aurora-diagnostics`, `aurora-runtime-*`: configuration, observability, and runtime assembly;
- `validation/`: end-to-end software validation that does not depend on a specific physical product.

See [`docs/architecture.md`](docs/architecture.md) and [`docs/PROJECT_SCOPE.md`](docs/PROJECT_SCOPE.md).

## Build and test

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Linux stable is the primary validation environment. Windows stable verifies the portable software surface, and CI also checks the declared Rust 1.78 MSRV.

## Immersive software validation

The JOC/IEC61937 validation lane is deliberately a **software test**, not a hardware claim. It pins the external Harletty and Omniphony revisions, exercises real E-AC-3 JOC metadata/object handling, checks a plain E-AC-3 negative control, and renders a 7.1.4 output file.

```bash
bash validation/immersive/test-joc-stack.sh
```

The gate is successful only when it emits all of these markers:

```text
PLAIN-EAC3-NEGATIVE-CONTROL-PASS
JOC-IEC61937-PASS
7.1.4 render PASS
AURORA JOC SOFTWARE STACK PASS
```

A separate experimental surround-upmix path exists for channel-based E-AC-3. It explicitly does not claim object recovery.

## Run examples

```bash
cargo run -p aurora-cli -- gains --layout stereo --steps 12
cargo run -p aurora-cli -- render --scene fixtures/scenes/circle.json --input fixtures/audio/mono_sweep.wav --output output/scene.wav
cargo run -p aurora-cli -- doctor
cargo run -p aurora-cli --all-features -- devices
cargo run -p aurora-cli --all-features -- realtime --output-device 0 --scene fixtures/scenes/asymmetric_5_1.json --sample-rate 48000 --block-size 256 --apply-geometric-delay --test-signal rotating-sine
```

## Integration rule

New platform work must enter Aurora through a generic adapter boundary. Core crates may not depend on a named phone, board, MCU, boot image, vendor firmware, or appliance filesystem.

Third-party integrations and their licensing status are documented in [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) and [`docs/adapters.md`](docs/adapters.md).
