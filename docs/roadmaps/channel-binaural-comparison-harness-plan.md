# Aurora Channel-to-Binaural Comparison Harness Plan

## Status

- Reference project: `ThreeDeeJay/HeSuVi-File-Virtualizer`.
- Decision: **adapt workflow ideas only; no code, preset, or runtime dependency adoption**.
- Tracking issue: `#53`.
- Legal status: no explicit repository license was found; implementation must be clean-room and Aurora-owned.
- Dependencies: `#44`, `#48`, and the provenance rules of `#52`.

## Objective

Build one deterministic offline pipeline that renders the same validated multichannel PCM through multiple Aurora binaural engines or admitted channel-to-binaural filter banks, then emits lossless masters, optional listening copies, objective reports, and optional remuxed comparison media.

This removes repeated manual work from HRTF, BRIR, filter-bank, renderer, and reference-engine evaluation.

## 1. Architectural boundary

The implementation must separate:

1. media demux/decode;
2. Aurora-owned PCM and channel-layout validation;
3. renderer or filter-bank execution;
4. objective analysis;
5. listening-copy generation;
6. optional external media remux.

FFmpeg may be used as an isolated external process for container handling. It must not define Aurora's audio model and must never run inside realtime callbacks.

## 2. Channel-to-binaural renderer

Implement a dedicated offline renderer for fixed speaker-bed input.

For every declared input channel `c`, the validated filter bank provides:

- `h_left[c]`;
- `h_right[c]`.

The output is the deterministic sum of each input-channel convolution into the corresponding ear.

Initial layouts:

- stereo;
- 5.1;
- 7.1.

Height layouts are admitted only when the filter bank declares complete and validated height-channel coverage.

This renderer is distinct from object-based directional HRTF rendering and must use separate capability names and public types.

## 3. Canonical channel layout

Every job must carry an explicit canonical channel map. Input with missing, duplicated, unsupported, or ambiguous labels fails before rendering.

The renderer must not infer 7.1 ordering from channel count alone.

Required tests include one impulse per channel so routing errors are visible independently for the left and right outputs.

## 4. LFE policy

LFE handling is explicit and recorded in the artifact manifest:

- omit;
- use a declared stereo LFE filter pair;
- low-pass and distribute using a documented gain;
- user-specified policy.

The default must never silently process LFE as a normal full-range speaker.

## 5. Filter-bank manifest

A versioned manifest must define:

- stable ID and version;
- supported layouts;
- channel order;
- left/right impulse asset per channel;
- sample rate and impulse length;
- delay and gain metadata;
- origin and measurement or derivation method;
- license, attribution, redistribution, modification, and commercial-use status;
- checksum and acquisition procedure;
- room/head/virtualizer identity;
- known limitations;
- admission status from the `#52` provenance gate.

Opaque commercial captures are comparison-only unless explicit rights permit more.

## 6. Sample-rate policy

Support three explicit offline modes:

1. exact rate match;
2. resample input PCM to the filter-bank rate;
3. resample filter impulses to the session rate after measured validation.

All resampling must be disclosed in the output manifest. Compare timing error, spectral error, CPU, and memory before selecting defaults.

## 7. Comparison runner

Target CLI surface:

```text
aurora-cli compare-binaural \
  --input source.mkv \
  --layout 7.1 \
  --engines geometric,aurora-hrtf,filterbank:<id> \
  --output-dir output/comparison/<run-id>
```

The runner must:

1. decode one canonical PCM source;
2. execute each selected renderer from identical PCM;
3. retain raw lossless stereo WAV masters;
4. create optional loudness-matched listening copies separately;
5. optionally remux labeled tracks to MKV;
6. write one manifest even when individual engines fail;
7. support deterministic resumption without recomputing accepted outputs.

## 8. Comparison fairness

Mandatory rules:

- same decoded PCM for every engine;
- no system-wide or hidden DSP;
- no lossy encoding before objective analysis;
- no destructive loudness normalization of raw masters;
- latency-aligned copies are separate from original-timing outputs;
- every applied gain or alignment offset is reported;
- clipping and non-finite output are failures, not corrected silently.

## 9. Objective evidence

Through issue `#44`, record:

- peak and RMS;
- integrated loudness where available;
- latency/onset estimate;
- ITD and ILD behavior;
- spectral difference and interaural correlation;
- clipping and finite-sample status;
- CPU time and peak memory;
- deterministic output checksum;
- renderer, filter-bank, dataset, config, and commit identities.

No single metric may be presented as a perceptual-quality score.

## 10. Blinded listening package

Optional output:

- randomized neutral labels;
- level-matched lossless excerpts;
- full-length tracks when appropriate;
- answer key stored separately;
- metadata stripped of engine identity.

Subjective results remain separate from CI and objective evidence.

## 11. Failure safety

- Never modify or delete source media.
- Use a run-scoped temporary directory.
- Write outputs atomically.
- Record per-engine success, failure, timeout, and checksum state.
- Preserve completed masters if optional remux or encoding fails.
- Allow resume only when source, configuration, engine version, and checksums match.

## 12. Explicit exclusions

Do not add:

- HeSuVi runtime dependency;
- Equalizer APO control;
- copied batch code;
- bundled HeSuVi HRIR presets;
- Windows-only system DSP behavior;
- a general-purpose media editor.

## 13. Execution order

1. `#44` artifact and benchmark foundation.
2. `#48` common media ingestion.
3. `#52` filter-bank provenance and legal admission.
4. Synthetic filter-bank renderer correctness.
5. One lawful real filter-bank fixture.
6. Multi-engine comparison runner.
7. Optional listening package and media remux.

## Acceptance

- Aurora-owned Rust implementation;
- deterministic channel routing and FIR output;
- explicit layout, LFE, rate, and normalization policies;
- raw masters preserved independently from listening copies;
- resumable failure-safe runs;
- machine-readable reports and checksums;
- no unlicensed code or filters;
- explicit validation against analytical fixtures and at least one lawful real filter bank.
