# Aurora Lean Audio Stack — minimal external runtime

Status: curated architecture and software-only CI; no 100% service/format or physical speed proof.

## Why the former list looks large

The 29 entries in config/external-components-v1.json are a research/test inventory, not 29 processes running on Aurora. Historical code and differential test sources may remain outside the audio deployment. The authoritative minimal selection is config/aurora-lean-audio-profile-v1.json. It is enforced by validation/streaming/aurora_lean_profile.py and negative regression tests.

## Selected deployment plan

| Audio function | Exactly one selected runtime group | Notes |
| --- | --- | --- |
| Ordinary channel-bed decoding and conversions | FFmpeg (6.1.6 fallback, 9.0.2 latest stable under validation) | AC-3/DD+ 5.1 encoded fixtures already validated; other codec families need targeted end-to-end evidence |
| Native immersive objects/3D render | Harletty + Omniphony, inseparable version-matched bundle | v0.8.0 + v0.6.0 candidate; no separate OpenJOC or additional VBAP renderer in the same object-render path |
| EQ/FIR/crossover | CamillaDSP 4.1.3 | Optional, only when room correction is actually required; avoid duplicated external EQ runtime |
| PCM bus and output | Existing Aurora Rust core | 12 channels S32_LE/48 kHz, ordered FL FR FC LFE SL SR SBL SBR TFL TFR TRL TRR |

Maximum three third-party runtime groups (four source packages), with optional DSP. No simultaneous fallback decoders. The proposed runtime remains unverified as a complete hardware product.

## Exactly one disabled specialist per unresolved family

- Dolby AC-4: OxideAV AC-4 0.0.8 ONLY as offline evaluation. It has documented unimplemented AC-4 cases; NOT a universally working playback module.
- MPEG-H: Fraunhofer mpeghdec as sole evaluated runtime candidate; Ittiam libmpegh only a differential test oracle.
- IAMF: libiamf as sole decode candidate; iamf-tools only generates/probes fixtures. iamf-tools 3.0 cannot be presented as an IAMF 2.0 decoder.
- xHE-AAC: no verified owner, fail closed.
- Dolby MAT: no verified HDMI MAT input/parser/object extractor; simple LPCM does not imply native Atmos.
- OpenJOC: differential JOC reference, not a second live bridge. JOCForge: fixtures only. EBU and other renderers: offline comparison only.

## Best and fastest selection policy

Correctness and object truth first; speed second. Compare old/new binaries only on the SAME target CPU with the SAME encoded multi-channel fixture, same options and at least five measured repetitions. Use aurora_codec_perf.py for a DD+ 5.1 -> 12-channel offline throughput comparison. This is not measured live eARC latency, object-render quality, Wi-Fi synchronization or acoustic parity. No speed winner is established yet.

Every new component must REPLACE an accepted incumbent after accuracy, channel-order, crash/recovery, and same-host latency/performance benchmarks, OR cover a genuinely missing source format. No extra projects merely because a newer repo exists. Group-decoders with tied ABIs must update atomically, old version available for rollback; live audio callback must never swap libraries.

## Mandatory software checks

- aurora_lean_profile.py validates one chosen decoder owner for every declared codec family.
- test_lean_profile.py deliberately injects duplicate decoders, wrong object renderers and unverified specialists and expects rejection.
- test_component_upgrades.py checks seven-stage API/rollback simulation.
- Streaming CI exercises real encoded AC-3/DD+, LPCM, and FFmpeg throughput smoke.

Physical verification and real process manager integration remain separate required gates. No Dolby certification or universal Netflix/Disney+/Prime Video claim.
