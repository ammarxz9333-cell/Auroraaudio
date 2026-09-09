# Third-Party Licenses

Aurora keeps third-party components behind crate, adapter, process, or configuration boundaries where practical. The proprietary `crates/aurora-decoder-engine/**` subtree does not relicense any dependency listed here.

## Decoder dependencies in the experimental decoder stack

| Dependency | Purpose | License posture |
| --- | --- | --- |
| OpenJOC 0.16.0 | Native E-AC-3 JOC admission, reconstruction and speaker rendering | Apache-2.0; third-party notices must be retained; no Dolby certification, trademark, or patent rights are implied |
| `oxideav-ac3` 0.0.11 | Native AC-3 / E-AC-3 decode | MIT |
| `oxideav-ac4` 0.0.8 | Native AC-4 decode, including upstream immersive/A-JOC-to-PCM paths | MIT; Aurora integration is experimental and does not yet export AC-4 object metadata at the Aurora boundary |
| `oxideav-core` 0.1 | Shared OxideAV packet/frame/decoder abstractions | MIT |
| FFmpeg | Compatibility fallback for codec families without an admitted native backend | External executable only; license depends on the shipped FFmpeg build (commonly LGPL-2.1-or-later, potentially GPL when configured with GPL components) |

## Other current Rust dependencies

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
| `tracing` | Logging facade for runtime diagnostics | Permissive Rust ecosystem dependency |

## Adapter candidates

| Adapter | Dependency status | License / risk posture |
| --- | --- | --- |
| CamillaDSP | Not vendored, not linked, external executable only | Review redistribution obligations before product shipment |
| IAMF reference/tooling | Not vendored, not linked, disabled by default | Preferred open immersive-audio candidate; review source license and AOM patent-license conditions before product use |
| `truehdd` | Not vendored into Aurora decoder engine; disabled by default | Apache-2.0 source candidate; codec patent/trademark/product-distribution posture still requires review and technical acceptance |
| Cavern | Not vendored, not linked, disabled by default | Disabled pending license review |
| Symphonia | Not integrated into the proprietary decoder engine by default | MPL-2.0 file-level copyleft; may be isolated/evaluated but Aurora proprietary files must remain separately licensed |

Codec implementations, standards documents, trademarks, certification programs, and codec-essential patents are separate legal layers. A permissive source-code license does not by itself grant all patent or trademark rights that a commercial media product may need.
