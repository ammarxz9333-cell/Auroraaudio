# Operational stack audit — 2026-10-04

Only executable paths may be selected for runtime. Planned references stay outside the build/selection path; a reference pin is not an adapter. This initial audit removes two confirmed status-only crates and corrects a false test claim. It is not a completed product acceptance audit.

| Function | Executable path | Evidence and limit | Required work |
| --- | --- | --- | --- |
| AC-3/E-AC-3 channel decode | Existing external FFmpeg surround worker | Real IEC61937 compatibility gate; synthetic heights | Continuous integration/clock handoff remains distinct from file execution |
| Raw TrueHD channel decode | Same worker with `--input-format truehd` | Generated 5.1 fixture, lossless round trip, zero bed difference locally; exact-pin CI newly required | Aurora runtime handoff and broader fixtures; no DAMF/Atmos objects |
| JOC object decode | Existing pinned streaming-decoder reference lane | `validation/immersive/test-joc-aurora-live-runtime.sh`; software reference only | Legitimate real TV/eARC JOC capture and full physical output chain |
| Speaker rendering | Aurora VBAP; explicitly selected native libspatialaudio adapter | Executable renderer and geometry/runtime gates | Full-path deadline/latency and acoustic quality measurements |
| Common DSP | Aurora prepared DSP/output path | Executable per-role output checks; external CamillaDSP reference differential | Physical output/correction validation |
| Wired endpoints | AVDECC-owned GenAVB six-stream runtime | Exact-pin lifecycle/fault gates and physical-evidence collection tools | Actual NXP/ESP clock/RX epoch and loopback acceptance |
| Wi-Fi endpoints | Worker-side AOO adapter; ESP candidate | Host UDP faults/drift/reconnect; ESP firmware build | RF/synchronization/latency acceptance and production selection |
| Head tracking | Bounded adapter, delivery simulation and scheduler bridge | Actual PCM commits, reconnect/reset, zero-allocation gates in #222 | Physical tracker timestamp/latency validation |
| <=5 cm speaker localization | ADR-0023 prototype proposal | No accepted physical localization implementation/result | Implement shared-clock measurement/solver, reject weak geometry and validate independent ground truth |
| Netflix Atmos | Authorized Netflix device -> compatible TV -> ARC/eARC -> Aurora ingress | No complete service/physical execution result | Supported player/TV negotiation, legitimate capture, verified compressed decoder, synchronized output and measured lip sync |

Removed `aurora-decoder-truehdd` and `aurora-renderer-cavern` implemented only unavailable-status constants. Nothing depended on them. The retained capability IDs make unsupported requests explicit without compiling empty adapters. No external pins or license provenance were deleted.

There is currently no evidence that Aurora is the fastest system, supports every format, or matches Samsung/Sonos acoustically. The integration target is one production decoder/render path per stream, one common DSP pass, a shared logical media timeline and timestamped fanout. Decoder frame/lookahead, output buffering and network resilience all add latency; callback speed alone is not end-to-end latency.

Netflix requires a supported playback device and Atmos-capable audio system: https://help.netflix.com/en/node/64066 . Sonos also documents the TV/ARC/eARC dependency: https://support.sonos.com/en-us/article/listen-to-dolby-atmos-audio-from-your-tv-on-sonos . Aurora's corresponding complete gate remains unproven; a generated TrueHD test is not a substitute.
