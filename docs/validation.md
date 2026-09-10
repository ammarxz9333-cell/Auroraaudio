# Aurora Validation Matrix

This document is the evidence boundary for the direct-eARC validation harness.
It deliberately separates software facts from physical-product claims. IEC61937
E-AC-3 data type `0x15` is transport evidence only; JOC admission remains a
complete-access-unit OpenJOC classifier decision.

## Status vocabulary

- **PROVEN**: already supported by executed evidence independent of the current
  GitHub-hosted runner, or a static format/contract fact directly pinned by code.
- **TESTED**: an automated deterministic test/gate exists on this branch. Where
  the exact-head GitHub runner has not executed, the row says so explicitly.
- **NOT-PROVEN**: requires evidence not available from this software-only harness.

A TESTED row is not automatically CI-green. GitHub-hosted Actions have recently
failed before step execution, so exact-head Cargo execution remains a separate gate.

## Validation commands

The branch exposes two complementary headless tools:

- `aurora-sim-source`: generate ordinary 48 kHz / 5.1 E-AC-3 with local FFmpeg or
  frame supplied raw `.ec3`, wrap complete AUs into Aurora's canonical IEC61937
  carrier, and inject deterministic transport faults.
- `aurora-sim`: run `channel-id`, `latency-report`, and `stress` validation against
  Aurora's canonical 7.1.4 software path. The stress defaults are 100 switch cycles,
  1000 pause/resume cycles and a 1800-second soak with a 5% RSS-growth ceiling.

The dedicated `.github/workflows/aurora-validation-ci.yml` gate generates a real
Cargo 1.85 lockfile, uploads it as an artifact, runs `cargo test --workspace`, runs
validation-specific check/test/clippy, executes short headless command smoke tests,
and gates the pinned OpenJOC synthetic positive-JOC path. The full 30-minute soak
is intentionally not mislabeled as CI-proven until it is run.

## Transport and framing

| Area | Status | Evidence / acceptance |
| --- | --- | --- |
| Canonical E-AC-3 IEC61937 preamble | PROVEN | Simulator writes Pa=`0xF872`, Pb=`0x4E1F`, Pc=`0x0015` in the byte order consumed by Aurora's parser. |
| E-AC-3 carrier geometry | PROVEN | Canonical harness geometry is 192 kHz, two S16 slots, 24,576 carrier bytes per repetition period = 32 ms. |
| E-AC-3 payload bound | TESTED | 24,560-byte payload round-trips; 24,561 bytes fail closed. Exact-head Cargo execution pending. |
| Word swap / odd payload padding | TESTED | Round-trip test validates native payload recovery and zero pad confined to the carrier word. Exact-head Cargo execution pending. |
| Arbitrary capture chunk boundaries | TESTED | Complete periods are fed using deliberately irregular read sizes and must recover byte-identical payloads with zero malformed headers. Exact-head Cargo execution pending. |
| Complete E-AC-3 AU framing | TESTED | `Eac3AccessUnitFramer` delegates to pinned OpenJOC `parse_access_unit_bounds()` and requires following-boundary or finite-EOS proof. Exact-head Cargo execution pending. |
| FFmpeg-generated 384/768/custom-rate E-AC-3 | TESTED | CI generates 384, 768 and 640 kbit/s 5.1 streams locally, wraps them and requires valid E-AC-3 bursts with zero malformed headers/cadence mismatches. Exact-head execution pending. |
| Corrupt Pa resynchronization | TESTED | Integration test corrupts Pa, drops only that burst, and requires recovery of the following byte-exact payload. Exact-head Cargo execution pending. |
| Dropped-burst gap | TESTED | Integration test inserts full idle periods so elapsed carrier time is retained and requires the following Pa offset to expand by the missing periods. Exact-head Cargo execution pending. |
| Parser-visible E-AC-3 -> LPCM interval -> E-AC-3 | TESTED | The encoded parser view is modeled as a full non-IEC idle carrier interval and must resynchronize to the next E-AC-3 burst. This does **not** prove raw LPCM input negotiation. Exact-head Cargo execution pending. |
| Truncated finite carrier / `--truncated-eof` | TESTED | Independent integration and runtime-backed tests require an explicit incomplete/truncated payload error, never a fabricated complete observation. Exact-head Cargo execution pending. |
| `--cut-burst` | TESTED | Finite cut is rejected by IEC61937 EOF validation. A runtime-backed regression also feeds a mid-payload cut followed by a new period and requires failure rather than silent decoded audio. If that regression fails when first executed, it is a core-bug reproduction gate; parser/engine must not be changed before the failure is examined. |
| Encoded payload byte loss | TESTED | IEC61937 may remain structurally parseable because it has no payload integrity field; damaged encoded content therefore requires downstream framing/decoder validation. This layering property is not presented as transport authentication. |
| Cadence jitter | TESTED | Integration and CLI gates insert deterministic idle carrier bytes and require the resulting Pa-to-Pa spacing change to remain visible. Exact-head Cargo execution pending. |

## JOC path

| Area | Status | Evidence / acceptance |
| --- | --- | --- |
| `0x15` is not treated as JOC proof | PROVEN | Direct-eARC bridge clears fixed codec hints and leaves JOC classification to the complete-AU decoder path. |
| Pinned OpenJOC JOC fixture exists | PROVEN | Pinned OpenJOC revision `e7e03bc834ac0483770933cdc50ac058b100d1e2` includes synthetic `crates/openjoc-wasm/testdata/joc.ec3`; Aurora pins its SHA-256 in validation CI. |
| Synthetic JOC fixture survives simulator transport | TESTED | Existing CI gate performs `.ec3 -> aurora-sim-source -> IEC61937 -> probe -> .ec3` and requires byte-for-byte equality. Exact-head CI execution pending. |
| Synthetic positive JOC admission | TESTED | Validation CI downloads the exact pinned upstream fixture and runs Aurora's ignored positive OpenJOC classifier/render regression. Admission is from the complete AU, not the transport type. Exact-head CI execution pending. |
| Synthetic canonical 7.1.4 speaker render | TESTED | Existing OpenJOC/Aurora tests require successful canonical 12-channel speaker output. Exact-head CI execution pending. |
| Positive JOC latency visibility | TESTED | Validation CI runs `aurora-sim latency-report` on the pinned JOC fixture and requires a nonzero `joc_render` sample count. Exact-head CI execution pending. |
| Ordinary FFmpeg E-AC-3 negative JOC path | TESTED | `aurora-sim` has a direct regression requiring ordinary locally generated E-AC-3 to remain non-JOC after complete-AU engine admission. Exact-head execution pending. |
| Commercial Netflix/streaming JOC interoperability | NOT-PROVEN | Requires a real licensed playback source and physical capture path. |
| Preservation/export of original object scene coordinates | NOT-PROVEN | Current OpenJOC integration is a speaker renderer; Aurora does not claim exported object-scene metadata from that path. |
| “Atmos proven” product claim | NOT-PROVEN | Synthetic JOC software evidence is insufficient for a commercial Atmos interoperability claim. |

## Pipeline and failure behavior

| Area | Status | Evidence / acceptance |
| --- | --- | --- |
| Direct S32-slot normalization -> carrier -> parser -> decoder -> SpeakerPostProcessor | TESTED | `aurora-sim stress` converts each generated canonical carrier period into two-slot S32 words and feeds the production `AuroraPlaybackRuntime`; output must remain finite canonical 12-channel PCM. Exact-head execution pending. |
| Codec transition retirement | TESTED | Previous decoder state is drained before reset so valid short PCM tails are not silently discarded. Exact-head CI execution pending. |
| Finite E-AC-3 truncation | TESTED | Checked AU framing, parser EOF validation and checked bed finalization reject incomplete finite input. Exact-head CI execution pending. |
| No crash / no silent corruption under defined injector cases | TESTED | Each injector has deterministic expected resync or explicit-failure behavior. Runtime cut-burst acceptance is a hard failure. Exact-head CI execution pending. |
| Canonical 7.1.4 layout remains unchanged | PROVEN | Harness obtains channel order directly from `StandardLayout::SevenOneFour.canonical_roles()`; it introduces no alternate Aurora layout. |
| Software channel-ID identity through SpeakerPostProcessor | TESTED | Twelve unique channel tones are injected one canonical lane at a time; the dominant output lane must equal the source lane. Exact-head execution pending. |
| Manual `channel-id` WAV speaker semantics | TESTED | WAVE_FORMAT_EXTENSIBLE data is reordered to ascending channel-mask speaker slots while the test sequence remains Aurora canonical. This avoids the otherwise incorrect SL/SR vs SBL/SBR labeling. Exact-head execution pending. |
| Physical speaker channel order | NOT-PROVEN | `aurora-sim channel-id` creates a 12-channel WAVE_FORMAT_EXTENSIBLE manual fixture, but physical cable/amplifier/speaker mapping still requires real output hardware. |
| Raw eARC LPCM source switching | NOT-PROVEN | Current encoded harness can represent the parser-visible non-IEC interval and canonical PCM output leg, but does not emulate/claim the raw LPCM capture negotiation path. |

## Realtime, latency and stress

| Area | Status | Evidence / acceptance |
| --- | --- | --- |
| Speaker DSP host timing baseline | PROVEN | Prior executed host self-test measured p50 11.1 us, p99 13.3 us and max 118 us per 40-frame DSP block. It excludes decode, transport and hardware. |
| Fixed-capacity latency recording | TESTED | `StageLatencyBook` records simulated capture-normalization, parser/decode/JOC-render/SpeakerPostProcessor/output-copy samples with fixed storage; allocation regression requires zero heap allocations during repeated `record()` calls. Exact-head Cargo execution pending. |
| `aurora-sim latency-report` | TESTED | Command measures simulated S32 capture normalization, parser, decoder calls, SpeakerPostProcessor and a preallocated sink copy; it imports OpenJOC render timing only after positive JOC classification. Physical ALSA/device I/O remains explicitly `NOT_MEASURED`. Exact-head smoke execution pending. |
| JOC decode/render timing visibility | PROVEN | Fixed-size live JOC health exposes last decode/render/total and max-total timing without formatting on the hot path. |
| Full physical capture -> output latency | NOT-PROVEN | Simulated capture normalization and sink-copy CPU time are not ALSA read/write or acoustic round-trip latency. |
| New harness hot-loop allocations | TESTED | Carrier period, S32 words, idle/jitter buffers, sink scratch and latency counters are allocated before the stress/report loops and reused. Out-of-band RSS reading is excluded from the audio hot path. Exact-head allocator/runtime execution remains pending. |
| Zero allocations for every complete production realtime stage | NOT-PROVEN | Existing runtime still owns additional outer containers/backends; whole-chain allocator activity needs target execution. |
| Stress defaults: 100 format/speaker-path cycles | TESTED | `aurora-sim stress` defaults to 100 5.1-canonical -> 7.1-canonical -> real E-AC-3 encoded path -> canonical LPCM speaker-path cycles. Raw LPCM ingress is not claimed. Full default run pending. |
| Stress defaults: 1000 pause/resume cycles | TESTED | Each cycle feeds an idle carrier interval, resets the production runtime and requires a subsequent valid E-AC-3 unit to resume. Full default run pending. |
| 30-minute memory stability <=5% RSS growth | TESTED | Linux stress samples `/proc/self/status` out-of-band after warmup and fails if peak RSS growth exceeds 5%. Full 1800-second run has **not** executed on current head. |
| Zero XRUNs / zero capture-queue starvation | NOT-PROVEN in file mode | These metrics are `N/A(file-mode)`, not fabricated zeroes. They require ALSA loopback or the physical capture/output rig. |
| CI smoke stress | TESTED | Dedicated validation workflow runs shortened switch/pause/soak settings headlessly to catch crashes, malformed PCM, channel swaps, unexpected fault acceptance and gross leaks. Exact-head workflow execution pending. |

## Physical acceptance still required

The software harness intentionally does not emulate HDMI/eARC electrical/link
negotiation. Final product acceptance still requires a physical eARC receiver and
Linux capture path, sustained real carrier cadence, target-host CPU/allocation
profiling, ALSA/TDM output with zero XRUNs/starvation over the acceptance window,
verified physical 7.1.4 channel order, and a real licensed streaming JOC source.
