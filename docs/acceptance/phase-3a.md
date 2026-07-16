# Phase 3A Milestone Evaluation

## Record

- Milestone: Phase 3A -- Deterministic Offline Spatial Rendering Improvements
- Final classification: `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`
- Execution state: `CLOSED`
- Evaluation date: `2026-07-16`
- Submitted implementation commit:
  `1d64cbd1030dda0ff464113110807f48031b6823`
- Evaluated implementation and defect-fix commit:
  `501fb9eea3c19284d71bd9d9bfa21e664f6e5a55`
- Evaluation record commit: the subsequent documentation commit containing this
  record, reported exactly in the pull request and final evaluation report
- Pull request: `#10`
- Base: `main-v2` at
  `bc2cf637bb8dc5d526a28271e1070efcff1e09d4`

A Git commit cannot contain its own hash. The evaluated code snapshot is pinned
above; the exact documentation commit is therefore recorded externally by Git,
the pull request, and the final report without a self-referential placeholder.

## Criteria

| Criterion | Result | Evidence and truth source |
| --- | --- | --- |
| Scope and architecture | PASS | New optional offline crate only; `code_review` |
| Existing `Renderer` trait unchanged | PASS | No API crate diff; `code_review` |
| Basic renderer and CLI/live defaults unchanged | PASS | No relevant implementation diff; `code_review` |
| Horizontal VBAP math and power normalization | PASS | Unit and fixture tests; `unit_test` |
| Deterministic pair selection and fallback | PASS | Repeated tests, wraparound and duplicate-angle tests; `unit_test` |
| Degenerate and invalid inputs | PASS | Empty, single-speaker, invalid-buffer, non-finite, and extreme-finite tests; `unit_test` |
| Canonical 5.1/7.1 order and scene-order independence | PASS | Offline fixture tests; `unit_test` |
| Finite deterministic output and continuity | PASS | Full-circle fixture test repeated three times; `unit_test` |
| Caller-owned bounded memory | PASS | Capacity and output-shape tests; `unit_test` |
| Zero allocation after warm-up | PASS | 1,000-call allocation audit and independent targeted rerun; `unit_test` |
| Production unsafe code | PASS | None introduced; test-only allocator uses `unsafe`; `code_review` |
| Public API documentation | PASS | Strict Rustdoc build; `build_validation` |
| Workspace quality gates | PASS | Format, Clippy, tests, Actionlint; `build_validation` |
| Release benchmark | PASS | Criterion completed; `host_api_observation` |
| Physical 5.1/7.1 and endpoint gates | PENDING | No hardware evidence; `physical_measurement` remains unavailable |

## Defects Corrected During Evaluation

- rejected non-finite enabled-speaker positions, gains, and delays at setup;
- made non-finite runtime state follow an explicit deterministic finite-silence
  fallback without steady-state allocation;
- used wider internal geometry and saturating finite conversion so extreme
  finite values cannot escape as NaN or infinity;
- added checked or saturating size arithmetic and structured output-shape
  coverage;
- added wraparound, duplicate-angle, single-speaker, non-finite, extreme-value,
  and invalid-buffer tests.

## Commands

```text
git status --short --branch
git log --oneline --decorate -15
git diff --stat main-v2...HEAD
git diff --check main-v2...HEAD
git rev-parse HEAD
git rev-parse main-v2
git show --no-patch simulation-sprint-1-accepted
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo bench --workspace
actionlint
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo test -p aurora-renderer-vbap --test offline_fixtures --quiet
cargo test -p aurora-renderer-vbap tests::warmed_up_render_allocates_zero_times -- --exact
```

The offline fixture command was run three consecutive times with identical
passing results.

## Results

- Tests: 130 passed, 0 failed, 5 explicitly ignored hardware-only tests.
- Rustdoc: passed with warnings denied.
- Actionlint: passed with no findings.
- Warmed-up renderer allocations: 0 over 1,000 calls.
- Dependencies: no new external dependency; the crate uses Aurora-owned crates
  and the existing workspace Criterion development dependency.

Criterion at 48 kHz, 256 frames, and one object:

| Renderer | Layout | Median estimate | Estimate interval | Block budget |
| --- | --- | --- | --- | --- |
| VBAP | 5.1 | 367.28 ns | 366.33--368.30 ns | 0.0069% |
| Basic inverse distance | 5.1 | 65.83 ns | 65.70--65.96 ns | 0.0012% |
| VBAP | 7.1 | 570.11 ns | 566.98--573.39 ns | 0.0107% |
| Basic inverse distance | 7.1 | 82.90 ns | 82.73--83.09 ns | 0.0016% |

These timings have truth source `host_api_observation`. They are not latency
measurements, p95 callback measurements, or physical hardware evidence.

## Scope Files

- `crates/aurora-renderer-vbap/`
- workspace membership in `Cargo.toml` and `Cargo.lock`
- `docs/phase-3a.md`
- `docs/architecture.md`
- `docs/roadmap.md`
- `docs/governance/hardware-blocked-parallel-development.md`
- `AURORA_MASTER_REFERENCE.md`

## Classification Reasoning

Every software, review, documentation, allocation, determinism, and benchmark
criterion passed. The authoritative Phase 3A dependency matrix also requires
physical 5.1/7.1 routing and channel identity, live endpoint behavior, physical
stability and clock behavior, and audible or speaker-dependent evidence. Those
gates are unavailable and were not replaced by simulation. Governance therefore
requires `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`, not `ACCEPTED` and not the
provisional `IMPLEMENTATION_COMPLETE_VALIDATION_PENDING`.

## Limitations And Exclusions

- horizontal two-dimensional VBAP only;
- no spread, elevation, HRTF, binaural, Ambisonics, or room modeling;
- no irregular-layout product or audible-quality claim;
- no live renderer selection or default change;
- no codec, HDMI, networking, wireless audio, GUI, AI, or calibration work;
- no physical routing, endpoint, clock, stability, latency, or listening result.

Phase 2 remains open and unchanged. PR `#7` remains separate and must not be
merged by this evaluation. No physical measurement was used, no accepted tag is
created for this conditional classification, and no Phase 3B or later milestone
was started.
