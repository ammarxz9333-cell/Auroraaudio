# Third-Party Licenses and R0 Integration Boundaries

Aurora's own repository code remains under the repository's MIT license. Aurora R0 is a private, non-commercial DIY appliance, but source-license boundaries are still recorded so the architecture is reproducible and does not accidentally relicense unrelated code.

## Active R0 software

| Project | R0 role | Integration | License posture |
| --- | --- | --- | --- |
| Harletty / `harletty-bridge` | E-AC-3 JOC decode and OAMD bridge | External source checkout / runtime bridge; not vendored | Project sources are Apache-2.0; the built bridge is documented upstream as GPL-3.0-or-later because it links Omniphony bridge components |
| Omniphony | 7.1.4 object/speaker renderer | External source checkout / runtime executable and libraries; not vendored | GPL-3.0-or-later upstream |
| VibesboxSRC | Reference evidence for SiI9437 I2S/IEC61937 capture behavior | Documentation/behavioral reference only; no Python source copied | MIT upstream |
| Linux / ALSA / PipeWire | Device and audio runtime | Operating-system components | Respective upstream licenses |

R0 currently pins Harletty commit `4ccedec804de3b29c02fb2a69575c2f49bf2fb37` and Omniphony commit `44acc87a9cbf4b5ac8f474f51d87851d2c642550` for repeatable validation.

The Aurora IEC 61937 parser is an independent Rust implementation of published IEC burst behavior and the hardware-observed SiI9437 capture representation. Its source comments credit the Vibesbox work that established those behavioral facts.

## Existing Rust dependencies

| Dependency | Purpose | License posture |
| --- | --- | --- |
| `anyhow` | CLI error reporting | permissive Rust ecosystem dependency |
| `clap` | CLI argument parsing | permissive Rust ecosystem dependency |
| `cpal` | local real-time audio backend | Apache-2.0 |
| `criterion` | development benchmarks | MIT OR Apache-2.0 |
| `crossbeam-queue` | bounded sample transport experiments | MIT OR Apache-2.0 |
| `hound` | WAV IO | Apache-2.0 |
| `rubato` | asynchronous sample-rate conversion | MIT |
| `serde` / `serde_json` | data/config serialization | permissive |
| `thiserror` | structured error types | permissive |
| `tracing` | diagnostics facade | permissive |

## Codec/trademark note

The private R0 build must not be described as Dolby-certified, HDMI-certified or an officially licensed Atmos product. Open-source code licenses and private-use scope do not grant trademark rights. Commercial distribution would require a new legal/licensing review and is explicitly outside R0.
