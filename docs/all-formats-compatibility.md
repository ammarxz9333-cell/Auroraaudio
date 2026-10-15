# Aurora universal audio formats and update-safe modular architecture

Date: 2026-10-11. STATUS: source-audited software design, NOT end-to-end TV/service/HDMI hardware proof.

## Universal format requirement: three independently tested layers

1. **Receiving:** an authorized HDMI/eARC source really sends the carrier/metadata and an actual capture adapter receives it. TV EDID/settings and DRM-compliant player output matter. The existing SiI9437 tap has a narrow proven DD+ IEC61937 test lane, NOT every HDMI HBR / multichannel PCM / MAT format.
2. **Decoding:** an independently tested version-pinned library extracts the actual channel bed and genuine dynamic object metadata when the format has objects. An arbitrary 12-channel output is not automatically native Dolby Atmos.
3. **Rendering:** Aurora maps decoded channel roles and object metadata to exactly 12 Aurora speaker slots: FL, FR, FC, LFE, SL, SR, SBL, SBR, TFL, TFR, TRL, TRR at 48 kHz, S32_LE.

NO universal decoder/receiver has been physically proven for all forms in this repo. Format candidates are NOT automatically installed/functional.

## Audited project matrix

| Codec / input family | Best upstream candidate | Aurora result and limitations |
| --- | --- | --- |
| AC-3 / DD+ channel PCM | FFmpeg 6.1.6 pinned | Actual encoded 5.1 fixtures decoded to synthetic-expanded 7.1.4 in CI; physical streaming input not tested |
| E-AC-3 JOC Atmos | Harletty v0.7.4 / Omniphony v0.5.2, OpenJOC reference | Native object synthetic JOC fixture, not real streaming apps |
| Expanded JOC / TrueHD Atmos / DTS:X / Auro-3D | Harletty v0.8.0 / Omniphony v0.6.0 | Version-matched upstream research candidate ONLY; CI separate bridge/host build; DTS:X extensions experimental |
| Multichannel raw LPCM | Aurora host PCM lane / FFmpeg | 2/6/8 decoded channel input tested; actual HDMI eARC LPCM capture unproven; PCM output lacks MAT object metadata |
| Dolby MAT | No verified Aurora MAT receiver/decoder | MISSING native object path; MAT must never be falsely treated as regular PCM or E-AC-3 |
| TrueHD (channel bed) | FFmpeg, Harletty | Backend candidate; actual eARC HBR capture and output not verified |
| DTS / DTS-HD | FFmpeg, Harletty | Backend candidate; IEC61937 carrier and eARC HBR physical tests missing |
| Dolby AC-4 / AC-4 A-JOC | OxideAV/oxideav-ac4 (new pure Rust project) | Announces support for PCM and object variants, but insufficient independently validated Aurora corpus/hardware evidence |
| MPEG-H | Fraunhofer-IIS/mpeghdec, Ittiam libmpegh | Aurora offline reference candidates; not native live eARC |
| IAMF | AOM libiamf and iamf-tools | Reference evaluation present, not a verified live HDMI path |
| AAC / HE-AAC / xHE-AAC | FFmpeg and system codec candidates | Verify format family/availability per source, no blanket native object claim |
| Opus / FLAC / ALAC / Vorbis / MP3 | FFmpeg | Media decoding candidates; often arrive from TV as decoded PCM rather than IEC bitstreams |
| Future formats, PCM more than 8 channels | Versioned external adapter | Block until correct labels, transport and target render verified |

Useful upstream sources:
- https://github.com/FFmpeg/FFmpeg
- https://github.com/harletty/harletty-bridge/releases/tag/v0.8.0
- https://github.com/mgth/Omniphony/releases/tag/v0.6.0
- https://github.com/OxideAV/oxideav-ac4
- https://github.com/Fraunhofer-IIS/mpeghdec
- https://github.com/AOMediaCodec/libiamf
- https://github.com/AOMediaCodec/iamf-tools
- https://github.com/DolbyLaboratories/daaplay (partial DAA/iOS AC-4 example; NOT an Aurora universal streaming decoder)

## Component upgrade boundaries

~~~text
Legitimate HDMI/eARC receiver and actual codec transport
 -> versioned ingress adapter: bounded packets + true observed format
 -> versioned decoder worker (in separate process)
 -> normalized channel bed OR verified object PCM/metadata
 -> versioned spatial renderer
 -> immutable Aurora 12ch/48k/S32LE PCM and channel-order contract
 -> versioned DSP/ASRC
 -> versioned speaker/output adapter with one or more clock domains
~~~

The native Rust aurora-decoder-api and aurora-runtime-materialization are existing implementation boundaries. The new Python contracts are control-plane validation prototypes and **NOT yet wired to manage the live Rust engine**. A frozen ABI must include a version number, codec/transport metadata, explicitly typed object claims, channel roles, epoch/generation id, sample rate/format, max packet/frame sizes, bounded latency and reset/mute/rollback behavior.

On a decoder crash / unsupported codec / stale generation: mute, flush and re-prime before new audio becomes audible. Do not run untrusted plugin callbacks, dynamic linking or updates within real-time audio callbacks.

### Version-pair safety

Harletty 0.8.0 requires Omniphony 0.6.0 and bridge_api 0.4. Aurora's currently pinned 0.7.4 + 0.5.2 is separate. New pair commits are pinned in config/aurora-codec-bundles-v1.json and its isolated candidate workflow. A mixed pair MUST FAIL.

Updates follow **stage -> integration test -> physical qualification -> mute/drain -> restart new worker -> verify shape/clock -> atomic activation -> keep previous -> rollback if fault**. A decoder worker restart may cause a brief mute; uninterrupted playback is a separate latency/real-hardware acceptance gate.

### Code added in this PR

- validation/streaming/aurora_format_routes.py: 21-family codec source registry with truth-preserving native vs synthetic labeling.
- validation/streaming/aurora_stream_epoch.py: software generation/format transition state machine; *not yet connected to running eARC CLI*.
- validation/streaming/aurora_lpcm_ingress.py: bounded host-side 2/6/8-ch audio format conversion, not a MAT decoder.
- validation/streaming/aurora_decoder_upgrade.py: pair lock, compatibility checks, fail-closed A/B policy, synthetic rollback tests. Real trusted CI/physical evidence issuance and actual subprocess control **still to implement**.
- config/aurora-codec-bundles-v1.json: pinned old paired versions and v0.8 research candidate.
- .github/workflows/candidate-joc-abi-ci.yml: candidate source build against the same Aurora JOC fixture without touching default stable manifest.

## Critical promotion gates

- Real source device/TV eARC handshake and observed bitstream/PCM/MAT negotiation.
- Independent codec corpus, PCM/channel isolation, object-metadata accuracy, crash recovery and stream-format changes.
- All 12 physical output slots and required wireless timing tested, not just rendered 12ch files.
- Long-duration XRUN/clock drift, seek/pause/resume, disconnected source/endpoint and malformed payload tests.
- No claim of every format being supported until tests on real selected hardware pass.

**No DRM circumvention. No silent stereo fallback reported as 7.1.4 native. No Dolby branding/certification claim from experimental code.**


## Per-stage update protection (added after source audit)

The frozen module chain is defined in config/aurora-audio-component-contracts-v1.json and verified by validation/streaming/aurora_component_contracts.py. All seven stages have typed input/output ABI names and the invariant 12-channel output. A future adapter update must match the previous and next interface exactly. Accidental stereo, mismatched role order, incompatible version, or dropping a stage is rejected before touching a running graph.

validation/streaming/aurora_component_supervisor.py models an operator-requested component upgrade transaction: stage the candidate without muting; require software and separately trusted physical approval; mute and flush before replacing; accept only authenticated CURRENT worker generation and exact input/output ABI, finite PCM, stable clock and no XRUNs; on failure increment epoch, discard the candidate and recover the previous graph while muted. An unverified recovery remains muted. Synthetic tests mutate all seven stages individually and exercise stale generations, bad channel order, bad clock and fail-closed rollback.

**Implementation boundary:** The Python modules are a testable CONTROL-PLANE reference only. They do NOT orchestrate actual processes, atomically replace binaries, read a physical mute line, or certify plugin signatures. The application/host adapter implementing process lifecycle and real acoustic/hardware verification remains pending. Safe software architecture is not the same as experimentally established system reliability.
