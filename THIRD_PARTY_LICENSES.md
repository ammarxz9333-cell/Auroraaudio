# Third-Party Licenses

Aurora keeps third-party components behind crate, process, or configuration boundaries where practical. Only dependencies and adapter candidates on the active product path belong in the current inventory; removed experiments remain available in Git history rather than as buildable packages.

## Current Rust dependencies

| Dependency | Purpose | License posture |
| --- | --- | --- |
| `anyhow` | CLI error reporting | Permissive Rust ecosystem dependency |
| `clap` | CLI argument parsing | Permissive Rust ecosystem dependency |
| `cpal` | Local real-time audio backend; Windows host support including WASAPI where available | Apache-2.0 |
| `criterion` | Release-mode microbenchmarks and statistical performance reports; development dependency only | MIT OR Apache-2.0 |
| `crossbeam-queue` | Benchmark/reference fixed-capacity sample transport; not the selected runtime transport | MIT OR Apache-2.0 |
| `hound` | WAV file reading and writing | Apache-2.0 |
| `rubato` 0.16.2 | Windowed-sinc adaptive asynchronous resampling behind Aurora-owned APIs | MIT |
| `serde` | Data-model serialization support | Permissive Rust ecosystem dependency |
| `serde_json` | JSON scene fixture loading and evidence formatting | MIT OR Apache-2.0 |
| `thiserror` | Structured library error types | Permissive Rust ecosystem dependency |
| `tracing` | Logging facade for control-plane diagnostics | Permissive Rust ecosystem dependency |

## Active adapter candidates

| Adapter | Dependency status | License / risk posture |
| --- | --- | --- |
| CamillaDSP | Not vendored or linked; external executable only | Functional offline process adapter; review license and redistribution obligations before product distribution |
| libiamf | Not vendored or linked; disabled by default | Preferred open immersive-audio decoder candidate; review codec, patent, and conformance posture before product use |
| libmysofa | Planned isolated native adapter; not yet added | Review LGPL obligations, data licensing, unsafe/FFI isolation, and redistribution policy before implementation |

## Removed experiments

The former Cavern and `truehdd` placeholder crates were deleted during structural cleanup. They were not functional dependencies and are not current adapter candidates. Reintroducing either technology requires a new legal and architecture review plus a functional implementation plan.

No Dolby, DTS, HDMI, HDCP, Cavern, `truehdd`, CamillaDSP, libiamf, libmysofa, or wireless-speaker dependency is required to build Aurora core. The CPAL backend remains isolated in `aurora-realtime-audio-cpal` and does not cross Aurora core APIs.
