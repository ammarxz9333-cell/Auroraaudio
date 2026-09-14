# JOCForge external validation reference

Aurora uses JOCForge only as a pinned external fixture generator and structural conformance reference. It is not part of Aurora core, is not a runtime encoder dependency, and is not evidence of Dolby certification or universal hardware compatibility.

## Pin

- Upstream: `https://github.com/chyinan/JOCForge`
- Commit: `05a4108e0c6288130dec1203b301979a91475fca`
- License: Apache-2.0
- Aurora config: `config/jocforge-reference-v1.json`

The pin is intentional. CI and evidence must not follow `main`, `latest`, or an unreviewed release implicitly.

## Why Aurora uses it

JOCForge exposes a broad, standards-derived E-AC-3/JOC composition space that is difficult to cover with a small set of hand-authored fixtures. Aurora will use selected deterministic vectors to test decoder and recovery semantics across profile, carriage, block-partition, dependent-substream, EMDF-signaling, and LFE-ownership boundaries.

The first Aurora lane is intentionally narrow:

1. build the pinned source in an isolated CI job;
2. generate a representative 12-20-vector smoke corpus;
3. record fixture hashes and generator provenance;
4. feed those fixtures independently through Aurora/Harletty and OpenJOC;
5. compare structural and rendering semantics rather than demanding sample-identical PCM;
6. promote no capability claim unless the corresponding Aurora evidence artifact is reproducible.

## Required PR-smoke coverage

The representative matrix must cover all five public profiles (`idx0` through `idx4`) and include raw/container carriage where supported, `6`, `3+3`, `2+2+2`, and six single-block partitions, zero/one/multiple dependent-substream cases, both independent and dependent LFE ownership, and strict/deployed EMDF signaling.

The smoke matrix is not the same thing as JOCForge's full modeled space. Aurora must not state that it validated all upstream structural variants unless an explicit exhaustive Aurora job actually does so.

## Differential acceptance

Each generated fixture should be evaluated through the applicable independent lanes. Comparison fields include frame count, duration, finite/non-silent PCM, channel activity, object identifiers where exposed, topology, positions, gains, LFE exclusion, and recovery behavior. PCM sample identity is not required across independent renderers.

Once the representative baseline is stable, a separate negative corpus may add truncated EMDF, malformed metadata, corrupt dependent headers, profile changes, discontinuities, timestamp jumps, and reconnect/reset cases. Those tests must prove bounded failure: no panic, no deadlock, no unbounded buffering, and deterministic fail-closed/recovery behavior.

## Truth boundary

Upstream JOCForge describes a bounded public-spec implementation and publishes its own interoperability evidence. Aurora does not inherit those claims automatically. In particular, Aurora does not infer production mastering quality, perceptual parity, lossless recovery of the authoring master, universal decoder/device interoperability, proprietary implementation equivalence, or certification from JOCForge.

JOCForge remains a generator/reference lane unless a later architecture decision explicitly changes that status after technical and legal review.
