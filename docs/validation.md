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

A TESTED row is not automatically CI-green. The current GitHub-hosted Actions
failure occurs before step execution, so exact-head Cargo execution remains a
separate gate.

## Transport and framing

| Area | Status | Evidence / acceptance |
| --- | --- | --- |
| Canonical E-AC-3 IEC61937 preamble | PROVEN | Simulator writes Pa=`0xF872`, Pb=`0x4E1F`, Pc=`0x0015` in the byte order consumed by Aurora's parser. |
| E-AC-3 carrier geometry | PROVEN | Canonical harness geometry is 192 kHz, two S16 slots, 24,576 carrier bytes per repetition period = 32 ms. |
| E-AC-3 payload bound | TESTED | 24,560-byte payload round-trips; 24,561 bytes fail closed. Exact-head Cargo execution pending. |
| Word swap / odd payload padding | TESTED | Round-trip test validates native payload recovery and zero pad confined to the carrier word. Exact-head Cargo execution pending. |
| Arbitrary capture chunk boundaries | TESTED | Two complete periods are fed using deliberately irregular read sizes and must recover byte-identical payloads with zero malformed headers. Exact-head Cargo execution pending. |
| Complete E-AC-3 AU framing | TESTED | `Eac3AccessUnitFramer` delegates to pinned OpenJOC `parse_access_unit_bounds()` and requires following-boundary or finite-EOS proof. Exact-head Cargo execution pending. |
| Corrupt Pa resynchronization | TESTED | Corrupted Pa suppresses that burst; the next valid Pa/Pb is recovered without a fabricated payload. Exact-head Cargo execution pending. |
| Dropped-burst gap | TESTED | Missing bursts are represented by full idle carrier periods so elapsed carrier time is retained; next Pa spacing expands by the dropped-period count. Exact-head Cargo execution pending. |
| Truncated finite carrier | TESTED | A declared E-AC-3 payload cut at EOF must return `TruncatedPayload`/an explicit incomplete-burst error. Exact-head Cargo execution pending. |
| Encoded payload byte loss | TESTED | IEC61937 may remain structurally parseable because it has no payload integrity field; the damaged E-AC-3 AU is then rejected by OpenJOC framing. This is a layering property, not silent acceptance by the decoder. Exact-head Cargo execution pending. |
| Cadence jitter | TESTED | File-mode simulator inserts deterministic idle carrier bytes; CI gate requires the resulting Pa-to-Pa mismatch to be reported. Exact-head Cargo execution pending. |

## JOC path

| Area | Status | Evidence / acceptance |
| --- | --- | --- |
| `0x15` is not treated as JOC proof | PROVEN | Direct-eARC bridge clears fixed codec hints and leaves JOC classification to the complete-AU decoder path. |
| Pinned OpenJOC JOC fixture exists | PROVEN | Pinned OpenJOC revision `e7e03bc834ac0483770933cdc50ac058b100d1e2` includes synthetic `joc.ec3`; upstream tests report eight decoded access units and JOC object metadata. |
| Synthetic JOC fixture survives simulator transport | TESTED | CI gate performs `.ec3 -> aurora-sim-source -> IEC61937 -> probe -> .ec3` and requires byte-for-byte equality. Exact-head CI execution pending. |
| Synthetic positive JOC admission | TESTED | Existing ignored Aurora fixture test requires positive OpenJOC JOC classification from a complete AU. Exact-head CI execution pending. |
| Synthetic canonical 7.1.4 speaker render | TESTED | Existing ignored OpenJOC/Aurora tests require successful canonical 12-channel speaker output. Exact-head CI execution pending. |
| Ordinary E-AC-3 negative JOC path | TESTED | Decoder policy does not promote transport type `0x15`; non-JOC E-AC-3 remains the bed path. Exact-head current-head execution pending. |
| Commercial Netflix/streaming JOC interoperability | NOT-PROVEN | Requires a real licensed playback source and physical capture path. |
| Preservation/export of original object scene coordinates | NOT-PROVEN | Current OpenJOC integration is a speaker renderer; Aurora does not claim exported object-scene metadata from that path. |
| “Atmos proven” product claim | NOT-PROVEN | Synthetic JOC software evidence is insufficient for a commercial Atmos interoperability claim. |

## Pipeline and failure behavior

| Area | Status | Evidence / acceptance |
| --- | --- | --- |
| Direct carrier -> parser -> decoder bridge | TESTED | Existing direct-eARC integration tests exercise the production bridge; simulator now supplies deterministic carrier input. Exact-head CI execution pending. |
| Codec transition retirement | TESTED | Previous decoder state is drained before reset so valid short PCM tails are not silently discarded. Exact-head CI execution pending. |
| Finite E-AC-3 truncation | TESTED | Checked AU framing and checked bed finalization reject incomplete finite input. Exact-head CI execution pending. |
| No crash / no silent corruption under defined injector cases | TESTED | Injector cases have deterministic expected resync or explicit failure behavior. Exact-head CI execution pending. |
| Canonical 7.1.4 layout remains unchanged | PROVEN | Harness uses the existing 12-channel Aurora canonical layout and does not introduce a new layout. |
| Physical speaker channel order | NOT-PROVEN | Requires real output hardware and speaker/channel observation. |

## Realtime, latency and stress

| Area | Status | Evidence / acceptance |
| --- | --- | --- |
| Speaker DSP host timing baseline | PROVEN | Prior executed host self-test measured p50 11.1 us, p99 13.3 us and max 118 us per 40-frame DSP block. It excludes decode, transport and hardware. |
| JOC decode/render timing visibility | PROVEN | Fixed-size live JOC health exposes last decode/render/total and max-total timing without formatting on the hot path. |
| Full capture -> parser -> decode -> render -> DSP -> output stage percentiles | NOT-PROVEN | Per-stage fixed-capacity aggregation and an executed target-host run are still required. |
| Zero allocations for every complete realtime stage | NOT-PROVEN | Several hot boundaries are pooled and have allocation regressions, but complete end-to-end allocator activity still needs measurement. |
| 30-minute zero-XRUN run | NOT-PROVEN | Headless file transport has no ALSA XRUN concept; physical/snd-aloop ALSA execution is required for an XRUN assertion. |
| 30-minute memory stability <=5% RSS growth | NOT-PROVEN | Stress runner and an executed long-duration result are required. |
| 100 format-switch cycles | NOT-PROVEN | Stress matrix execution result not yet available. |
| 1000 pause/resume cycles | NOT-PROVEN | Stress matrix execution result not yet available. |

## Physical acceptance still required

The software harness intentionally does not emulate HDMI/eARC electrical/link
negotiation. Final product acceptance still requires a physical eARC receiver and
Linux capture path, sustained real carrier cadence, target-host CPU/allocation
profiling, ALSA/TDM output with zero XRUNs/starvation over the acceptance window,
verified physical 7.1.4 channel order, and a real licensed streaming JOC source.
