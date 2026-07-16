# Phase 3B Milestone Evaluation

## Record

- Milestone: Phase 3B -- Deterministic Horizontal Source Spread and Irregular
  Layout Support
- Execution state: `CLOSED`
- Final classification: `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`
- Evaluation date: `2026-07-16`
- Governance commit: `6c4ce2070f29f488df81417ec429db01de2b3f10`
- Governance PR: `#11`, merged normally into `main-v2`
- Implementation branch base:
  `59e99f1ad2f89fd7fa03658b8a698d5dd9baf5b2`
- Final implementation commit:
  `4dd7f30125cbbdeba4273b7a560085f6534c5232`
- Evaluation evidence commit:
  `04a37dca069e90c707954f88093fc1ba2858325b`
- Evaluation record commit: the subsequent documentation commit containing this
  record, reported exactly in the pull request and final report

The evaluated code and evidence snapshots are pinned above. A commit cannot
contain its own hash, so Git and the pull request identify the subsequent record
commit without a self-referential placeholder.

## Criteria

| Criterion | Result | Evidence and truth source |
| --- | --- | --- |
| Phase 3A point compatibility | PASS | Bit-exact spread-zero unit and canonical fixture tests; `unit_test` |
| Deterministic spread law | PASS | Domain, participation, tie, and repeated-render tests; `unit_test` |
| Irregular-layout correctness | PASS | Uneven 10-channel fixture and permutation tests; `unit_test` |
| Power normalization | PASS | Complete spread/source sweep with `1e-4` tolerance; `deterministic_simulation` |
| Finite output and bounded samples | PASS | Non-finite fallback, extreme finite, silence, and sample-product checks; `unit_test` |
| Stable tie-breaking | PASS | Wraparound, midpoint, duplicate, near-duplicate, exact-hit tests; `unit_test` |
| Layout-order independence | PASS | Identifier-mapped reversed layout tests; `unit_test` |
| Sparse and dense layouts | PASS | One/two-speaker and 12/16-speaker tests/benchmarks; `unit_test` |
| Caller-owned bounded memory | PASS | Existing scratch reused in place; capacity audit; `unit_test` |
| Zero warmed-up allocation | PASS | 0 allocations over 1,000 spread renders on 16 speakers; `unit_test` |
| Public API documentation | PASS | Strict Rustdoc with warnings denied; `build_validation` |
| Protected contracts and defaults | PASS | No diff in API, Basic, CLI, realtime, simulator, or Cargo manifests; `code_review` |
| Deterministic fixtures | PASS | Three repeated runs; quantized checksum `0ef1fc03dfa5892e`; `deterministic_simulation` |
| Host benchmark | PASS | Focused and workspace Criterion completed; `host_api_observation` |
| Physical hardware gates | PENDING | No physical evidence was collected; `physical_measurement` unavailable |

## Commands

```text
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
actionlint
cargo bench --workspace
cargo test -p aurora-renderer-vbap --test offline_fixtures --quiet
cargo test -p aurora-renderer-vbap tests::warmed_up_spread_render_allocates_zero_times -- --exact
```

The deterministic fixture command ran three consecutive times.

## Results

- Workspace tests: 144 passed, 0 failed, 5 explicitly ignored hardware-only
  tests.
- Phase 3B coverage: 26 VBAP unit tests and 6 offline integration tests.
- Warmed-up spread allocations: 0 over 1,000 calls.
- Irregular sweep: 21 spread values by 721 source angles, repeated with
  cross-platform quantized checksum `0ef1fc03dfa5892e`.
- Dependencies: none added.
- Production unsafe: none added; allocation instrumentation remains test-only.

Focused Criterion at 48 kHz, 256 frames, and one object:

| Scenario | Median | Estimate interval | Budget |
| --- | --- | --- | --- |
| Point 5.1 | 368.42 ns | 367.50--369.43 ns | 0.0069% |
| Point 7.1 | 550.66 ns | 548.41--553.14 ns | 0.0103% |
| Intermediate 5.1 | 1.0982 us | 1.0955--1.1012 us | 0.0206% |
| Intermediate 7.1 | 1.6811 us | 1.6781--1.6841 us | 0.0315% |
| Maximum irregular-10 | 2.7889 us | 2.7823--2.7962 us | 0.0523% |
| Intermediate dense-16 | 6.8552 us | 6.8247--6.8884 us | 0.1285% |

Criterion did not provide p95 or maximum callback timing. Point 5.1 was about
0.31% slower and point 7.1 about 3.41% faster than Phase 3A evaluation medians,
which is not a material point-path regression. Workspace results showed host
load variation in unrelated benchmarks. Every timing above is
`host_api_observation`, not latency or physical evidence.

## Protected-Contract Audit

- Aurora-owned `Renderer` trait: unchanged.
- Basic renderer and CLI/live defaults: unchanged.
- Callback ownership, state, fault, device, and truth semantics: unchanged.
- Scratch: existing one-float-per-speaker caller-owned capacity reused.
- Render path: no allocation, blocking, logging, formatting, filesystem, or
  process access.
- Phase 2, accepted simulator, Phase 3A record, and tags: unchanged.

## Classification Reasoning

All software, deterministic, host-observation, documentation, and review gates
passed. The approved matrix still requires audible spread verification on
identifiable speakers, physical 5.1/7.1 routing, endpoint behavior, real-path
level consistency, and hardware stability. Those gates were not replaced with
simulation. Governance therefore requires
`CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`, not `ACCEPTED`.

## Limitations And Exclusions

- horizontal spread only; no elevation or 3D/triplet VBAP;
- no HRTF, binaural, Ambisonics/HOA, room behavior, Doppler, or head tracking;
- no calibration, hardware integration, or physical validation;
- no CLI/default renderer selection change;
- no audible-quality, physical routing, endpoint, level, stability, or latency
  claim.

Phase 2 remains open and PR `#7` remains separate and unmerged. Phase 3A point
behavior remains intact. No physical measurement was performed, no accepted tag
is created for this conditional classification, and no Phase 3C or later
milestone was started.
