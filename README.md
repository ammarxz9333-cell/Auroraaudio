# Aurora

Aurora is an open, modular, **hardware-agnostic immersive-audio software stack** written primarily in Rust.

Aurora's core owns audio/scene models, rendering, DSP, realtime scheduling, simulation, validation, and generic I/O boundaries. The software core has no canonical hardware target. Physical transports and devices may be integrated only through optional adapters and must never become prerequisites for the engine.

## Project direction

Aurora is software-first. The active goals are:

- open scene and channel/object processing boundaries;
- offline and realtime multichannel rendering, including 7.1.4-class layouts;
- pluggable decoder/renderer/DSP backends;
- deterministic simulation and repeatable validation;
- generic local audio I/O through portable APIs;
- explicit capability reporting so documentation cannot claim more than the code proves.

Hardware integrations, when needed, live outside the software core behind generic adapter contracts. No specific device or product defines Aurora's architecture.

Aurora also does not claim Dolby certification, DRM circumvention, or production compatibility with proprietary streaming services.

## Capability truth registry

The canonical capability state is a versioned typed registry owned by `aurora-core`. This compact table is generated from that registry and is checked in CI.

<!-- AURORA_CAPABILITIES_BEGIN -->

| Capability | Implementation | Offline | Realtime | Verification | Tested layouts | Production-ready |
| --- | --- | --- | --- | --- | --- | --- |
| `geometric-binaural` — Geometric binaural renderer | experimental | functional | experimental | ci-artifact | stereo | no |
| `iamf` — IAMF rendered-PCM decoder | functional | functional | unsupported | ci-artifact | stereo | no |
| `camilladsp` — CamillaDSP external adapter | functional | functional | unsupported | software-tested | 5.1 | no |
| `cavern` — Cavern renderer research candidate | inactive-research | unsupported | unsupported | none | none | no |
| `truehdd` — truehdd channel-PCM decoder | experimental | experimental | unsupported | software-tested | none | no |
| `loudspeaker-3d` — 3D VBAP loudspeaker renderer | experimental | functional | experimental | software-tested | 7.1.4 | no |

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

Aurora also tracks pinned OpenJOC as an **independent reference backend**, kept outside the Aurora Rust workspace. Its fail-closed reference lane requires positive JOC admission before accepting a 7.1.4 render, and the differential lane runs the same pinned fixture through OpenJOC and the Harletty/Omniphony path and records duration, level, and non-gating correlation evidence.

```bash
bash validation/immersive/test-openjoc-reference.sh INPUT_JOC
bash validation/immersive/test-joc-differential.sh
```

These OpenJOC lanes are evaluation evidence only; they do not make OpenJOC an Aurora core dependency or establish hardware eARC, proprietary-streaming compatibility, certification, or production readiness.

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

New platform work must enter Aurora through a generic adapter boundary. Core crates may depend only on portable software contracts, not on product-specific assumptions.

Third-party integrations and their licensing status are documented in [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) and [`docs/adapters.md`](docs/adapters.md).
