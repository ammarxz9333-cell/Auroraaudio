# JOCForge external validation reference

Aurora uses JOCForge only as a pinned external fixture generator and structural conformance reference. It is not part of Aurora core, is not a runtime encoder dependency, and is not evidence of Dolby certification or universal hardware compatibility.

## Pin

- Upstream: `https://github.com/chyinan/JOCForge`
- Commit: `05a4108e0c6288130dec1203b301979a91475fca`
- License: Apache-2.0
- Aurora config: `config/jocforge-reference-v1.json`

The pin is intentional. CI and evidence must not follow `main`, `latest`, or an unreviewed release implicitly.

## Why Aurora uses it

JOCForge exposes a broad, standards-derived E-AC-3/JOC composition space that is difficult to cover with a small set of hand-authored fixtures. Aurora uses selected deterministic vectors to test decoder and recovery semantics across profile, carriage, block-partition, dependent-substream, EMDF-signaling, and LFE-ownership boundaries.

The first Aurora lane is intentionally narrow:

1. build the pinned source in an isolated CI job;
2. generate a representative 12-20-vector smoke corpus;
3. record fixture hashes and generator provenance;
4. feed those fixtures independently through Harletty and OpenJOC;
5. compare structural and rendering semantics rather than demanding sample-identical PCM;
6. promote no capability claim unless the corresponding Aurora evidence artifact is reproducible.

## Current PR-smoke corpus

The current representative matrix contains 14 vectors:

- source-derived profile baselines: `idx0`, `idx1`, `idx2`, `idx3`, `idx4`;
- block partitions: `6`, `3+3`, `2+2+2`, and `1+1+1+1+1+1`;
- two-dependent topology;
- deployed-EMDF short/dependent topology;
- independent LFE;
- dependent-LFE supplementation and replacement.

The generated corpus also covers strict/deployed EMDF signaling and phase-signaling profiles. The matrix is intentionally representative, not exhaustive.

The deterministic synthetic ADM/BW64 fixture generated in the current CI lane has SHA-256:

`dd2c349fb232c8aaa0eceff6c8b4602466cfae9ec163949be8211a5a996b8add`

## Observed interoperability on the pinned matrix

### OpenJOC 0.17.0

The five source-derived baseline profiles all pass OpenJOC inspection and full 7.1.4 rendering. For each baseline, OpenJOC reports a decoder-admissible continuous stream and produces 12-channel, 48 kHz float PCM in this order:

`FL, FR, FC, LFE, Lb, Rb, Ls, Rs, TFL, TFR, TBL, TBR`

The nine structural probes are retained as classifications rather than promoted to render-support claims. Eight currently classify as `stream_parse=pass`; `partition-6` classifies as `stream_parse=fail` and `decoder_admissible=false`. That finding remains visible in evidence and is not converted into an allow-failure pass.

The deployed-EMDF short/dependent probe is also intentionally recorded with deployed compatibility passing while ETSI-strict compatibility fails.

### Harletty v0.7.4

Using the pinned Harletty bridge at commit `10943821cca7e6886c11f45d2267b06d76e6db7c` and raw host-sized 4096-byte chunks:

- all five baseline profile streams are accepted and emit 32 decoded frames each;
- all nine structural probes also terminate and emit decoded PCM frames;
- no bridge error is reported for any of the 14 vectors;
- none of the 14 vectors exposes object metadata through this bridge path;
- the current observed outcome for every vector is therefore `pcm-only`;
- the evidence summary records `object_metadata_vectors = 0`.

The baseline streams also produce repeated `eac3_dependent_queue_overflow dropping oldest pending dependent frame` diagnostics under this host-chunking path. The streams still terminate and produce the expected decoded frame count, but the diagnostic is a compatibility observation that must not be hidden.

Therefore this JOCForge lane currently establishes **Harletty decoder acceptance / PCM classification**, not Harletty JOC object-scene reconstruction for this corpus. OpenJOC remains the independent semantic/render reference for the generated baseline vectors.

This result does not contradict separate Aurora tests that validate Harletty object metadata on other reviewed fixtures. Capability claims must remain fixture- and path-specific.

## Differential acceptance

Each generated fixture is evaluated through the applicable independent lanes. Comparison fields include frame count, duration, finite/non-silent PCM, channel activity, object identifiers where exposed, topology, positions, gains, LFE exclusion, and recovery behavior. PCM sample identity is not required across independent renderers.

For the current JOCForge corpus, object-level Harletty comparisons are not available because the pinned bridge exposes no object metadata on these vectors. That absence is retained as evidence rather than inferred away.

Once the representative baseline is stable, a separate negative corpus may add truncated EMDF, malformed metadata, corrupt dependent headers, profile changes, discontinuities, timestamp jumps, and reconnect/reset cases. Those tests must prove bounded failure: no panic, no deadlock, no unbounded buffering, and deterministic fail-closed/recovery behavior.

## Evidence outputs

`JOCForge Reference CI` writes deterministic evidence for:

- generator provenance and per-vector hashes;
- OpenJOC inspection/render classifications;
- per-vector Harletty decoder outcomes;
- the consolidated Harletty matrix summary.

The first fully passing 14-vector run after outcome-driven Harletty classification produced workflow artifact ZIP SHA-256:

`86735b2e84afe775fd1f50eedb3d69b07a6f5bced0496ce91976ae9641c09ac3`

Workflow artifacts are retention-limited CI evidence; the reproducible source pin, manifests, scripts, and truth boundaries are the durable project record.

## Truth boundary

Upstream JOCForge describes a bounded public-spec implementation and publishes its own interoperability evidence. Aurora does not inherit those claims automatically. In particular, Aurora does not infer production mastering quality, perceptual parity, lossless recovery of the authoring master, universal decoder/device interoperability, proprietary implementation equivalence, or certification from JOCForge.

The current Aurora evidence proves full OpenJOC 0.17.0 inspection plus 7.1.4 rendering for the five baseline vectors, role-appropriate structural classification for the other nine vectors, and bounded Harletty PCM decoding/classification for all 14. It does **not** prove Harletty object reconstruction for this corpus, exhaustive JOCForge coverage, physical eARC behavior, protected-service compatibility, proprietary equivalence, or certification.

JOCForge remains a generator/reference lane unless a later architecture decision explicitly changes that status after technical and legal review.
