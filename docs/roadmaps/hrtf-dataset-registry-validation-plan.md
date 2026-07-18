# Aurora HRTF/BRIR Dataset Registry and Validation Plan

## Status

- Tracking issue: `#52`.
- Reference reviewed: `ThreeDeeJay/BinauralDatabase`.
- Decision: use it as a discovery and metadata reference only.
- Do not vendor its Git LFS backups or treat catalog inclusion as a license grant.
- Required before true HRTF is accepted across more than one hand-picked dataset.

## 1. What BinauralDatabase provides

BinauralDatabase is a monthly Airtable backup exported to JSON, NDJSON, and YAML. It indexes a broad range of binaural resources, experiments, implementations, presets, and links. It is not one coherent HRTF dataset and it is not a rendering engine.

Its main value to Aurora is taxonomic and investigative:

- discovering candidate HRTF, HRIR, and BRIR datasets;
- identifying commercial and historical binaural implementations for comparison;
- finding research experiments and source publications;
- exposing the diversity of formats and metadata Aurora must handle.

The visible backup is stale relative to current development and must not be treated as an authoritative current registry.

## 2. Aurora-owned manifest

Each admitted dataset must have a versioned manifest containing:

- stable dataset ID and version;
- canonical source and publication;
- asset type: HRTF, HRIR, BRIR, ambisonic binaural filters, channel-to-binaural IR, or comparison preset;
- format and convention;
- sample rate, impulse length, receiver count, and spatial resolution;
- subject type and receiver/head model;
- measurement environment;
- coordinate convention and units;
- license, attribution, redistribution, modification, and commercial-use status;
- acquisition policy and checksum;
- known limitations and validation status.

## 3. Legal and provenance admission states

Every candidate receives exactly one state:

- `redistributable_fixture`;
- `download_for_ci`;
- `user_supplied_only`;
- `comparison_only`;
- `rejected`.

Public availability is not sufficient. Opaque commercial-product captures and presets associated with Dolby, DTS, Windows Sonic, Razer, Realtek, games, or headphone virtualizers remain comparison-only unless explicit rights are documented.

## 4. Dataset validator

Validation occurs outside realtime and must include:

- checksum and file integrity;
- dimensions, receiver count, and finite samples;
- sample rate and delay metadata;
- coordinate-system normalization;
- direction coverage, duplicates, and missing directions;
- left/right consistency;
- impulse onset, DC offset, clipping, and truncation checks;
- canonical direction lookup generation;
- explicit resampling policy.

Malformed or ambiguous datasets must fail before renderer construction.

## 5. Minimal lawful reference corpus

Aurora should select a small diverse corpus rather than import the catalog:

- one permissively licensed mannequin HRTF dataset;
- one permissively licensed human-subject dataset when consent and redistribution terms permit;
- one permissively licensed BRIR dataset;
- one sparse dataset for interpolation stress tests;
- synthetic analytical fixtures for exact tests.

Large datasets remain user supplied or downloaded on demand. CI uses only small checksum-locked fixtures with documented provenance.

## 6. Validation matrix

The corpus must exercise:

- coordinate conversion;
- exact-direction lookup;
- nearest-neighbor and interpolated lookup;
- azimuth, elevation, and front/back differentiation;
- expected ITD and frequency-dependent ILD trends;
- spectral continuity during source movement;
- sample-rate conversion;
- deterministic convolution output;
- malformed-input rejection;
- dataset switching and reset behavior.

## 7. Required artifacts

Issue `#44` infrastructure must produce:

- impulse and frequency-response data;
- ITD/ILD trajectories;
- inter-direction spectral distance;
- interpolation discontinuity scores;
- front, rear, side, overhead, and moving-source WAVs;
- CPU and memory reports by impulse length and dataset size;
- source version, checksum, manifest, license, and commit metadata.

Objective results do not prove externalization or localization quality. Subjective listening tests must be randomized, level matched, blinded where practical, and reported separately from CI evidence.

## 8. Relationship to Aurora HRTF work

- Issue `#46` owns SOFA/HRIR loading and the native/unsafe boundary.
- Issue `#52` owns dataset provenance, registry, admission policy, normalization, and validation corpus.
- The offline HRTF renderer consumes only validated normalized datasets.
- Realtime HRTF follows only after offline correctness and bounded resource evidence pass.

## 9. Explicit exclusions

- no BinauralDatabase runtime dependency;
- no vendored Airtable or Git LFS backup;
- no callback-time download, parsing, or resampling;
- no unverified claim that a preset is a measured HRTF;
- no unreviewed commercial HRIR or virtualizer capture shipped with Aurora;
- no single-dataset success presented as general HRTF correctness.
