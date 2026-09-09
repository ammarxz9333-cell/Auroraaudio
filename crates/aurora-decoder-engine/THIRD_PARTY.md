# Aurora Decoder Engine — Third-Party Boundary

`aurora-decoder-engine` is proprietary orchestration code. It does not relicense third-party codec implementations.

## Integrated today

- OpenJOC — Apache-2.0. Native Rust E-AC-3 JOC decode/reconstruction/render integration through `aurora-decoder-open`.
- OxideAV `oxideav-ac3` — MIT. Native Rust AC-3/E-AC-3 decode through `aurora-decoder-open`.
- OxideAV `oxideav-ac4` — MIT. Experimental native Rust AC-4 integration. The Aurora adapter accepts Annex-G sync-framed input (including CRC verification) or packetized raw AC-4 with an explicit hint, converts S16 to Aurora F32, and reblocks to the configured Aurora block size. Mono/stereo are mapped conservatively; the documented immersive 7.1.4 lane order is normalized into Aurora canonical order. A-JOC/direct-object presentations currently fail closed because upstream object-signal PCM is not a speaker layout and Aurora has not yet exported the associated object/OAMD metadata into its scene renderer. Unknown multichannel layouts also fail closed rather than risking a channel-order error.
- FFmpeg — normally LGPL-2.1-or-later unless built with GPL components. Aurora invokes it as a replaceable external worker and does not vendor or relabel FFmpeg code.

## Preferred native candidates

- `truehdd/truehdd` — Apache-2.0. Candidate for native TrueHD and Atmos metadata extraction. Current `truehd` requires Rust 1.88 and the full `truehdd` package requires Rust 1.95, while Aurora currently targets Rust 1.85; therefore it stays isolated until the toolchain boundary is resolved.
- OxideAV `oxideav-dts` — MIT / Rust 1.80. Candidate for DTS Core only; current upstream explicitly excludes DTS-HD extension substreams, so it must not be claimed as DTS-HD MA or DTS:X support.
- OxideAV AAC/Opus family — MIT. Candidates for replacing compatibility-worker decode on common codecs after corpus/fuzz/performance gates.
- AOM IAMF tools — permissive source terms plus an AOM patent license. Candidate for IAMF after legal-notice and patent-condition review.

## Deliberately not absorbed into the proprietary core

- Symphonia — MPL-2.0. May be evaluated behind an adapter, but proprietary Aurora source must not be copied into or combined with modified MPL-covered source files.
- FDK-AAC — custom Fraunhofer license and separate patent concerns. Not admitted as a default dependency.
- Fraunhofer MPEG-H decoder — license review required before product integration.

## Product rule

A backend is promoted from candidate to integrated only when all of the following exist:

1. pinned upstream version/commit;
2. license and patent notice review;
3. deterministic conformance corpus versus at least one independent reference;
4. corruption/fuzz/panic-safety tests;
5. realtime latency and allocation evidence;
6. channel-order and metadata-fidelity tests;
7. explicit fallback behavior that never fabricates immersive objects.

Third-party names and marks remain the property of their respective owners. No inclusion in this catalog implies certification, endorsement, trademark rights, or patent clearance.
