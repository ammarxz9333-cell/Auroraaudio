# Third-Party Licenses

Aurora keeps third-party components behind crate, adapter, process, or configuration boundaries where practical. A project license is not a patent, trademark, certification, codec-pool, or redistribution clearance; product distribution still requires review of the exact binaries and notices being shipped.

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
| `serde_json` | Data-model serialization support | MIT OR Apache-2.0 |
| `thiserror` | Structured library error types | Permissive Rust ecosystem dependency |
| `tracing` | Logging facade for future runtime diagnostics | Permissive Rust ecosystem dependency |

External adapters, validation tools, and candidates:

| Component | Aurora boundary | License / risk posture |
| --- | --- | --- |
| FFmpeg | External process for channel-bed decode, resampling, carrier creation, and format conversion | External tool; distribution must follow the exact FFmpeg build's configured license and enabled components. Aurora does not vendor FFmpeg. |
| Harletty / `harletty-bridge` | Runtime plugin in immersive software validation; not part of Aurora core | Upstream project license is Apache-2.0. Keep exact-version notices and codec/patent review separate from the source-code license. |
| Omniphony | External renderer process used only by the pinned immersive validation lane | GPL-3.0. Do not copy or link Omniphony code into Aurora's permissively licensed core without an explicit GPL-compliance and distribution decision. External validation remains isolated. |
| OpenJOC | Independent external-process reference backend; future versioned C-ABI evaluation only | OpenJOC core is Apache-2.0; release/integration bundles can carry additional notices. Aurora currently does not vendor it or add it to the Rust workspace. |
| JOCForge | Exact-commit external fixture/conformance generator only; not linked into Aurora core and not a runtime encoder dependency | Source/workspace crates are Apache-2.0. Aurora pins `05a4108e0c6288130dec1203b301979a91475fca` and treats generated vectors as test inputs only. Source licensing does not establish patent, trademark, proprietary-format equivalence, device interoperability, or certification clearance. |
| AOMedia OAR | Exact-commit external reference build/test lane only; not linked into Aurora core and not yet an Aurora rendering backend | Source is BSD-3-Clause-Clear, but the upstream repository also carries the separate Alliance for Open Media `PATENTS` license. Any distribution or implementation decision must preserve and review both. Passing Aurora's OAR reference lane only qualifies the pinned upstream as an oracle; it does not establish IAMF integration, patent clearance outside the published terms, Dolby equivalence, or certification. |
| Fraunhofer IIS `mpeghdec` | Exact-commit external MPEG-H decode/render validation oracle only; not vendored or linked into Aurora core | The Fraunhofer FDK MPEG-H Software License permits source/binary redistribution subject to its conditions but explicitly grants **no patent license**. The Aurora lane pins commit `4448b69738da2fa5f2f2f2b0ce29eea32509e046` and uses it only for CI evidence. Product or service use requires separate patent/license review. Aurora does not redistribute the CI-built decoder binary. |
| Ittiam `libmpegh` | Exact-commit external MPEG-H decode/render oracle and public fixture source only; not vendored or linked into Aurora core | The source license is BSD-like, while `LICENSE2` explicitly states that no patent rights are granted and that additional third-party patent licenses may be required. Aurora pins commit `f7ff0ac78d4d83f0b853bf2dff2ef075c92724f8`; CI consumes the public fixture in place and does not redistribute the decoder binary or fixture as an Aurora artifact. |
| CamillaDSP | Not vendored, not linked, external executable only | Functional offline process adapter; review license and redistribution obligations before product use. |
| libiamf | Not vendored, not linked, disabled by default | Preferred open immersive-audio decoder candidate; review codec and patent posture before product use. |
| `truehdd` | Not vendored, not linked, disabled by default | Experimental/offline-only; high commercial risk pending legal review. |
| Cavern | Not vendored, not linked, disabled by default | Disabled pending license review; high commercial risk until cleared. |

No Dolby, DTS, HDMI, HDCP, Cavern, `truehdd`, CamillaDSP, libiamf, Harletty, Omniphony, OpenJOC, JOCForge, AOMedia OAR, Fraunhofer `mpeghdec`, Ittiam `libmpegh`, FFmpeg, or wireless-speaker dependency is required to build or test Aurora core. Optional validation lanes may require explicitly pinned external tools. The CPAL backend is isolated in `aurora-realtime-audio-cpal` and is not exposed through Aurora core APIs.
