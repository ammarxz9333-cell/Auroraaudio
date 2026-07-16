# Simulation Assurance Campaign 1 Evaluation

## Record

- Milestone: Simulation Assurance Campaign 1 -- Massive Deterministic Property
  and Stress Testing
- Execution state: `CLOSED`
- Final classification: `ACCEPTED`
- Evaluation date: `2026-07-16`
- Governance merge: `1b28d514882d407446dac3a366ecd35841a2a95a`
- Evaluated implementation: `a04d009752622ffe30b3b2fa24043aaa56a11344`
- Implementation branch: `test/simulation-assurance-campaign-1`
- Evaluation record commit: the subsequent documentation commit containing
  this record, identified by Git and the implementation pull request

The campaign is test-only and has no physical acceptance criterion. Its
acceptance does not close Phase 2 or any physical gate of Phase 3A or Phase 3B.

## Criteria

| Criterion | Result | Evidence and truth source |
| --- | --- | --- |
| Bounded deterministic generation | PASS | One live scenario, hard iteration caps, stable IDs and seeds; `code_review` |
| Replay and compact failure evidence | PASS | Single-scenario replay and 32-record bounded sibling report; `unit_test` |
| Panic, deadlock, and loop freedom | PASS | All bounded runs terminated without panic or hang; `deterministic_simulation` |
| Finite output and bounded state | PASS | 1K, 10K, 100K, and soak reports passed; `deterministic_simulation` |
| Zero warmed-up allocations | PASS | Existing callback and renderer allocation guards passed; `unit_test` |
| Lifecycle and fault observability | PASS | Valid transitions, explicit fault effects, and six legacy fixtures; `deterministic_simulation` |
| Channel and renderer invariants | PASS | Isolation, normalized energy, spread-zero compatibility, and permutation equivalence; `deterministic_simulation` |
| Required dimensions | PASS | Rates, callbacks, drift, jitter, fill, faults, layouts, spread, wraparound, duplicates, and invalid/extreme inputs; `deterministic_simulation` |
| Target-qualified reproducibility | PASS | Fixed 1K seeds repeated three times with one checksum; `deterministic_simulation` |
| Required campaign levels | PASS | PR, nightly, deep 100K, and accelerated 24h/7d/30d levels executed or provisioned as required; `deterministic_simulation` |
| Bounded CI workflows | PASS | PR, nightly, deep, and soak workflows pass Actionlint; `build_validation` |
| Protected contracts and dependencies | PASS | No public trait, production callback, simulator, renderer, or external dependency changed; `code_review` |
| Evidence terminology | PASS | Reports use simulation/host truth sources only; no physical result claimed; `code_review` |

## Executions

| Execution | Scenarios | Legacy | Checksum | Host seconds | Result |
| --- | ---: | ---: | --- | ---: | --- |
| Smoke | 1,000 | 0 | `a3b2c451ec78b20c` | 2.124 | PASS |
| Standard | 10,000 | 6 | `b7b8d8e97d2a3b70` | 21.777 | PASS |
| Fixed smoke, repeat 3 | 1,000 per repeat | 0 | `a3b2c451ec78b20c` each | 6.300 | PASS |
| Manual deep | 100,000 | 6 | `aea2c21b490b7973` | 207.930 | PASS |
| Accelerated 24h | 4 profiles | 0 | `60d8260f653090c9` | 4.519 | PASS |
| Accelerated 7d | 4 profiles | 0 | `f9a773a0e96180c6` | 31.122 | PASS |
| Accelerated 30d | 4 profiles | 0 | `3e746f9174106183` | 133.400 | PASS |

Checksums above are qualified for Windows x86-64. Host duration is
`host_api_observation`, not audio latency or physical evidence.

Generated reports are under `output/simulation-assurance/` and remain ignored:

- `smoke-1000.json`
- `standard-10000.json`
- `repeat-1000.json`
- `deep-100000.json`
- `soak-24h.json`
- `soak-168h.json`
- `soak-720h.json`

Each passing report retained one generated scenario at a time, at most 32
failure records, zero bytes of report growth per passing scenario, and a
maximum reported ring capacity of 32,768 frames. Report files remained about
2--4 KiB regardless of scenario count.

## Validation Commands

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
actionlint
cargo bench --workspace
cargo run --release -p aurora-simulation-assurance -- --level smoke --scenarios 1000 --start-seed 0 --report output/simulation-assurance/smoke-1000.json
cargo run --release -p aurora-simulation-assurance -- --level standard --scenarios 10000 --start-seed 0 --report output/simulation-assurance/standard-10000.json
cargo run --release -p aurora-simulation-assurance -- --level smoke --scenarios 1000 --start-seed 0 --repeat 3 --report output/simulation-assurance/repeat-1000.json
cargo run --release -p aurora-simulation-assurance -- --level deep --scenarios 100000 --start-seed 0 --report output/simulation-assurance/deep-100000.json
cargo run --release -p aurora-simulation-assurance -- --level soak --scenarios 4 --soak-hours 24 --start-seed 0 --report output/simulation-assurance/soak-24h.json
cargo run --release -p aurora-simulation-assurance -- --level soak --scenarios 4 --soak-hours 168 --start-seed 0 --report output/simulation-assurance/soak-168h.json
cargo run --release -p aurora-simulation-assurance -- --level soak --scenarios 4 --soak-hours 720 --start-seed 0 --report output/simulation-assurance/soak-720h.json
```

Workspace validation completed with 150 passed tests, zero failures, and five
explicitly ignored hardware-only tests. Strict Rustdoc, Actionlint, and all
workspace benchmarks passed. Benchmarks showed no material regression.

## Defect And Regression Evidence

The first accelerated 24-hour run exposed a generator defect: independently
selected clocks could exceed the simulator's supported relative mismatch. The
generator now bounds valid mismatch to 250 ppm and retains prolonged 500 ppm
as an expected structured rejection. Deterministic regression tests cover both
the valid generator bound and the unsupported prolonged mismatch. No product
engine defect was found.

## Limitations

- Failure reduction isolates one deterministic scenario; there is no generic
  automatic value shrinker.
- Floating-point checksums are target-qualified, not cross-platform promises.
- Accelerated duration represents simulated time, not wall-clock endurance.
- No endpoint, physical clock, audible behavior, routing, level, stability, or
  latency measurement was performed.

No second simulator, renderer feature, protected-contract change, Phase 3C
work, or later milestone was introduced. Phase 2 remains open and incomplete.
