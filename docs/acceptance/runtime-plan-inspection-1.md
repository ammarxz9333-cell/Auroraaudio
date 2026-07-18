# Runtime Plan Inspection 1: Final Architectural Evaluation

## Record

- Milestone: Runtime Plan Inspection 1
- Milestone class: software-only control-plane inspection
- Acceptance date: `2026-07-18`
- Authorization state: `AUTHORIZED`
- Milestone status: `COMPLETE`
- Execution state: `CLOSED`
- Evaluation classification: `ACCEPTED`
- Final decision: `ACCEPTED`
- Completed checkpoints: A, B, C, D
- Evaluated repository state:
  `0487a830df8bca0c479d39f69ea70597abfc7559`
- Checkpoint D branch: `docs/runtime-plan-inspection-1-checkpoint-d`

This decision accepts only the bounded, read-only inspection projection and its
deterministic formatting contracts. It does not accept a runtime subsystem,
hardware behavior, or physical evidence.

## Governance Basis

- [ADR 0016](../adr/0016-runtime-plan-inspection-boundary.md);
- [milestone specification](../planning/runtime-plan-inspection-1.md);
- [inspection format documentation](../runtime-plan-inspection.md);
- repository architecture, roadmap, master-reference invariants, and accepted
  runtime-assembly public contracts.

## Evaluated History

| Scope | PR | Reviewed head | Merge commit |
| --- | ---: | --- | --- |
| Governance | #29 | `2e98dd19917bdb4d0a25a8358250b53ba465825f` | `02deb93374b162d11b18b4010452254f3ecd1c18` |
| Checkpoint A | #30 | `ce150a738a8c6be013e6c4723dc647667763825c` | `b847344c8a64e2b605aead3b6cef8979f39b9916` |
| Checkpoint B | #31 | `1e2b8aacc519aa9d9481a146abd167889a79bd51` | `6c28ab826a40e84d3fcdbc07999a0414dce1b1ca` |
| Checkpoint B reconciliation | #33 | `c1aa27543bf50697e58439890ee9c1362ac0d561` | `5c75c934e669b54ded1e5c2422cba80c6c0073b0` |
| Checkpoint C | #32 | `f946a5d5abf4f0088e6ebbb99058196770661b61` | `c3bb059396185eed5155e1147eca5f35c8c15ac7` |
| Checkpoint C reconciliation | #34 | `5d1abed3732b87f68284cb5956c7fbcbdfb241ad` | `0487a830df8bca0c479d39f69ea70597abfc7559` |

No history or accepted tag was rewritten.

## Scope Evaluated

The review covered the separate `aurora-runtime-inspection` leaf crate, its
versioned immutable inspection-owned model, projection from borrowed
`PreparedRuntimePlan` and `PreparedSetupPlan` values, canonical source order,
bounded validation, structured errors, redaction, deterministic compact JSON,
deterministic text, fixed conformance findings, exact-output fixtures, public
documentation, dependencies, and prohibited scope.

No substantive code defect was found during Checkpoint D, and no code correction
was made.

## Dependency Matrix

| Dependency | Classification | Result |
| --- | --- | --- |
| `aurora-runtime-assembly` | Sole production Aurora dependency | PASS |
| `serde` | Private implementation dependency | PASS |
| `serde_json` | Private implementation dependency | PASS |
| `aurora-config` | Dev-only fixture dependency | PASS |
| Reverse dependency from another crate | Forbidden | None |
| Configuration, diagnostics, CLI, renderer, DSP, backend, engine, simulator, CPAL, scene, audio I/O, network, or host production dependency | Forbidden | None |

The accepted runtime-assembly and other protected crates are unchanged from the
governance base. No Serde implementation or derive was added to prepared-plan
types.

## Public API Summary

The crate exposes inspection-owned projection values, options, limits, errors,
`InspectionReport::project`, `JsonFormatter::format`, and
`TextFormatter::format`. Public signatures contain Aurora-owned and standard
Rust types only. Strict rustdoc passes with warnings denied.

The public API does not expose generic prepared-plan serialization,
deserialization, reconstruction, persistence, hashing, fingerprints,
signatures, cache identity, host access, or runtime construction.

## Output Bounds

| Bound | Value |
| --- | ---: |
| `MAX_JSON_BYTES` | 262144 |
| `MAX_TEXT_BYTES` | 262144 |
| `MAX_NESTING_DEPTH` | 8 |
| `MAX_SERIALIZED_COLLECTION_ENTRIES` | 256 |
| `MAX_FINDINGS` | 32 |
| `MAX_STRING_BYTES` | 256 |
| `MAX_TOTAL_STRING_BYTES` | 32768 |
| `MAX_CHANNELS` | 32 per direction |
| `MAX_ROUTES` | 64 |
| `MAX_SPEAKERS` | 32 |
| `MAX_SETUP_STAGES` | 6 |
| `MAX_SETUP_DEPENDENCIES` | 9 |

Checked arithmetic protects cumulative string, collection, and text-output
accounting. JSON writes through a bounded writer. Violations return structured
errors; no semantic record is silently truncated and no partial output is
returned as success. Maximum valid shapes and deterministic oversized failures
are covered by tests.

## Validation Results

| Command or audit | Result | Actual evidence |
| --- | --- | --- |
| `git diff --check` | PASS | No whitespace error |
| `cargo fmt --all --check` | PASS | No formatting change required |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS | Zero warning |
| `cargo test --workspace --all-features` | PASS | 254 passed; 5 hardware-only tests ignored |
| Exact-output, escaping, and repeated determinism tests, three runs | PASS | Every run byte-identical and successful |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps` | PASS | Public documentation warning-free |
| `cargo +1.78.0 check --workspace --all-targets --all-features` | PASS | MSRV check successful |
| `cargo bench --workspace` | PASS | Completed with exit code 0 |
| `actionlint` | PASS | Workflow syntax valid |
| Dependency and reverse-dependency scans | PASS | Required edge only; no reverse edge |
| Protected-contract and accepted-crate scans | PASS | No protected crate changed |
| Serialization/reconstruction/identity scans | PASS | Inspection-owned serialization only |
| Prohibited-scope and unsafe scans | PASS | No forbidden production capability or unsafe code |
| Public-surface and vocabulary scans | PASS | Documented inspection-owned API; semantic exclusions preserved |
| Relative Markdown-link validation | PASS | No broken relative link |
| Remote Checkpoint D PR checks | NOT_APPLICABLE at record creation | No final Checkpoint D commit or PR existed yet; no remote success is claimed |

The benchmark command is regression evidence from the local host, not physical
latency evidence. Criterion reported mostly no-change or improvement and flagged
roughly 2-3% regressions in several unrelated renderer/DSP comparisons. This
milestone changed no renderer, DSP, callback, or product behavior, and has no
inspection-specific benchmark. The observations are retained as host variability
and are not a blocking inspection-contract defect.

## Exact-Output Evidence

- Redacted and explicit-unredacted JSON fixtures match exactly.
- Redacted and explicit-unredacted text fixtures match exactly.
- Equal plans and options produce equal reports.
- Equal reports produce byte-identical compact JSON and identical text.
- Field, collection, enum, float, setup-stage, dependency, and finding order is
  fixed.
- Quote, forward-slash, backslash, control-character, and Unicode escaping is
  covered exactly.
- Non-finite floats and oversized output return structured errors.

## Acceptance Criteria

| # | Result | Evidence |
| ---: | --- | --- |
| 1 | PASS | Production Aurora edge is only inspection to runtime assembly; Serde remains private |
| 2 | PASS | No accepted public contract, behavior, schema, record, or tag changed |
| 3 | PASS | Equal prepared inputs produce byte-identical JSON and identical text |
| 4 | PASS | Canonical routes, outputs, speakers, stages, and dependencies remain ordered |
| 5 | PASS | Requested, prepared, deferred, and absent semantics remain distinct; no runtime evidence inferred |
| 6 | PASS | Default redaction covers authorized categories while preserving structure and presence |
| 7 | PASS | Maximum shapes succeed; overflow and oversized output fail without panic or truncation |
| 8 | PASS | No plan deserialization, reconstruction, hash, fingerprint, signature, or cache identity exists |
| 9 | PASS | No unsafe production Rust or external-state capability exists |
| 10 | PASS | Tests cover plan kinds, renderers, layouts, inactive state, devices, DSP, capacities, bounds, errors, and redaction modes |
| 11 | PASS | Strict rustdoc documents every public API and schema boundary |
| 12 | PASS | Formatting, Clippy, tests, rustdoc, Rust 1.78, Actionlint, scans, and repeated deterministic tests pass |
| 13 | PASS | No runtime execution, protected-contract change, Phase 2/3C work, physical claim, or forbidden integration exists |

Every mandatory criterion passes. No criterion is `FAIL` or
`NOT_APPLICABLE`.

## Semantic And Redaction Assessment

Reports represent requested intent, prepared control-plane facts, explicit
deferred values, and absence only. JSON keys, text labels, findings, fixtures,
rustdoc, and documentation make no negotiated, observed, simulated, measured,
runtime-ready, endpoint-health, latency, or physical claim.

Default projection uses deterministic category-specific markers for device,
channel, and speaker identifiers while preserving presence, counts, ordering,
route references, roles, active state, and dependencies. Unredacted local output
requires the explicit `InspectionOptions::unredacted_local()` option.

## Scope Intentionally Excluded

- prepared-plan serialization, deserialization, reconstruction, persistence,
  hashing, fingerprints, signatures, or cache identity;
- CLI or diagnostics producer integration;
- filesystem, environment, networking, host, device, clock, randomness,
  process, or thread access;
- renderer, DSP, backend, stream, callback, or engine construction/execution;
- Phase 2, Phase 3C, hardware validation, and physical or latency claims;
- schema expansion, additional consumers, and runtime readiness.

## Known Limitations

- Inspection schema 1 is a review projection, not the wire format of either
  prepared plan and cannot reconstruct one.
- Explicit unredacted local output can reveal identifiers by caller choice.
- Reports are bounded and reject larger future plan shapes until separately
  reviewed limits or schema versions are authorized.
- Inspection describes prepared control-plane intent only; it cannot prove
  runtime readiness, endpoint support, health, routing, hardware behavior, or
  latency.
- Final PR remote checks were not available when this record was authored and
  must not be reported as passed unless GitHub later confirms them.

## Terminal Decision

`ACCEPTED`

Runtime Plan Inspection 1 is complete and closed as a software-only milestone.
No runtime executed, no hardware was validated, and no physical claim is made.
Phase 2 remains open and incomplete. Phase 3C remains not started. This decision
does not authorize another milestone, implementation, integration, acceptance
tag, or release.

STOP. No later milestone was started.
