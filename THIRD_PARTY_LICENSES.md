# Third-Party Licenses

Aurora keeps third-party components behind crate, adapter, process, or configuration boundaries where practical.

Current Rust dependencies:

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
| `serde_json` | JSON scene fixture loading | MIT OR Apache-2.0 |
| `thiserror` | Structured library error types | Permissive Rust ecosystem dependency |
| `tracing` | Logging facade for future runtime diagnostics | Permissive Rust ecosystem dependency |

Adapter candidates:

| Adapter | Dependency status | License / risk posture |
| --- | --- | --- |
| CamillaDSP | Not vendored, not linked, external executable only | Functional offline process adapter; review license and redistribution obligations before use |
| libiamf | Not vendored, not linked, disabled by default | Preferred open immersive-audio decoder candidate; review codec and patent posture before product use |
| truehdd | Not vendored, not linked, disabled by default | Experimental/offline-only; high commercial risk pending legal review |
| Cavern | Not vendored, not linked, disabled by default | Disabled pending license review; high commercial risk until cleared |

No Dolby, DTS, HDMI, HDCP, Cavern, `truehdd`, CamillaDSP, libiamf, or wireless-speaker dependencies are required to build or test Aurora core. The CPAL backend is isolated in `aurora-realtime-audio-cpal` and is not exposed through Aurora core APIs.
