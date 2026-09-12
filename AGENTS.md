# Aurora Agent Reference — Living Handoff

> **Read this file first before changing Aurora.** This is the canonical compact handoff for coding agents. It exists to avoid re-reading old chats, re-discovering the repository, or repeating architectural decisions.
>
> **Maintenance contract:** every meaningful code, schema, architecture, validation, issue/PR-state, or critical-path change MUST update this file in the same commit/PR. Do not finish a task with this file stale.

Last project-state update: **2026-09-12**  
Canonical integration branch: **`main-v2`**  
Merged software baseline before this reference-file update: **PR #128**, merge `f3e0b605e22cb6232afcd98b828dab636fc52345`  
Current primary open architecture task: **Issue #119**  
Current WIP branch: **`feature/config-renderer-component-ref-v2`**

---

## 1. How a new agent should resume Aurora in under five minutes

1. Read this file completely.
2. Verify `main-v2` and the WIP branch heads on GitHub; SHAs in this file describe the last known state and may have advanced.
3. Read the active GitHub issue before editing. Right now that is **#119: versioned replaceable component references in configuration**.
4. If continuing the current WIP, inspect `feature/config-renderer-component-ref-v2` before starting a new branch.
5. Read only the subsystem docs relevant to the task; do not reload the whole project history.
6. Preserve the PROVEN / SOFTWARE-ONLY / TO-TEST distinctions below.
7. Before opening a PR, delete every temporary validation workflow created only for implementation/debugging.
8. Run the required validation gates.
9. Update this file with: current branch/task, exact blocker or completion, proof status, new important paths/contracts, PR/issue state, and the next action.

If the user says **“كمل” / “continue”**, continue the **Current WIP / Next action** section below. Do not restart architecture discovery unless the code contradicts this file.

---

## 2. Product goal

Aurora is an **open, modular, hardware-agnostic immersive-audio software stack**, primarily Rust.

Long-term target:

- real-time multichannel playback, initially 7.1.4-class and expandable toward 11.1.4;
- replaceable decoder, renderer, DSP, source, audio-I/O, transport, and hardware adapters;
- deterministic simulation and strong software evidence;
- eventual authorized TV/eARC ingestion and modular physical output hardware;
- later private wireless speaker/rear/multi-room networking;
- no AVR/soundbar dependency in the architecture;
- core software must not be tied to a specific SBC, MCU, DAC, eARC board, speaker product, or OS image.

Canonical scope/architecture references:

- `README.md`
- `VISION.md`
- `docs/PROJECT_SCOPE.md`
- `docs/architecture.md`
- `docs/plugin-architecture.md`
- `docs/configuration.md`
- `THIRD_PARTY_LICENSES.md`

---

## 3. Non-negotiable architecture and safety rules

- Core crates must remain hardware-agnostic.
- Platform integrations enter through narrow Aurora-owned adapter contracts.
- Realtime callback-reachable code must remain allocation-free after preparation, lock-free, free of formatting/logging, and free of filesystem/process access.
- Caller-owned/fixed buffers are preferred on renderer/DSP steady-state paths.
- Configuration/control-plane state is not runtime evidence.
- Configured, estimated, simulated, timestamp-derived, or synthetic latency must never be described as physically measured latency.
- Object decoding, channel decoding, and synthetic upmixing are distinct modes. Never silently substitute one for another.
- Unknown/incompatible component IDs, schema versions, capabilities, source generations, or contract versions fail closed.
- Prepare/validate/commit is transactional. A failed candidate must not mutate the active runtime/source.
- Application plugins are out-of-process and do not execute in the realtime callback.
- Never add DRM circumvention, Widevine bypass, protected-media extraction, or device-identity spoofing code.
- Do not claim Dolby certification or proprietary streaming-service compatibility from software CI.
- Do not copy/link incompatible third-party source into Aurora core. External GPL/reference implementations stay isolated.

---

## 4. Evidence truth table

### PROVEN / merged software behavior

- Rust workspace builds and tests across the established CI matrix.
- Deterministic simulation assurance exists, including PR smoke and deeper lanes.
- Accelerated sustained realtime health soak exists for 7.1.4-class processing.
- `RealTimeEngine` accepts **prepared renderer + prepared realtime delay/DSP components** through generic contracts.
- Production realtime-engine coupling to concrete Basic renderer/DSP implementations was removed by the #118 work; concrete Basic implementations may remain dev/test fixtures.
- Basic and VBAP renderer materialization use the same prepared engine boundary.
- A second `RealtimeDelayProcessor` test adapter uses the same prepared-component boundary.
- Failed replacement preparation is tested not to mutate the already active engine.
- Stable prepared component identities and compatible contract versions are owned by runtime assembly and appear in runtime inspection as **prepared control-plane intent**.
- Software JOC/IEC61937 validation lanes exist with pinned external Harletty/Omniphony references.
- OpenJOC is used as an independent fail-closed reference/differential lane; it remains outside the Aurora Rust core.

### HISTORICAL HARDWARE PROOF — limited scope

A prior lab chain demonstrated:

`Lindy 38368 / SiI9437 eARC -> Raspberry Pi 5 I2S slave -> IEC61937 type 0x15 @ 192 kHz -> raw E-AC-3 -> FFmpeg 5.1 PCM`

This proves only that the tested chain could extract/decode DD+/E-AC-3 5.1 in that setup. It does **not** prove full Atmos/JOC object recovery, 7.1.4 realtime rendering, production hardware, or streaming-service compatibility.

### NOT YET PROVEN / do not overclaim

- physical authorized eARC -> continuous E-AC-3 JOC -> Aurora -> physical 7.1.4 end-to-end realtime operation;
- commercial Netflix/other DRM service Atmos compatibility through Aurora;
- physical STM32/TDM16/USB multichannel output path;
- final DAC/amplifier/speaker electrical design;
- acoustic calibration and Samsung-Q995-class listening/performance parity;
- final private wireless speaker network latency/sync/reliability;
- thermal/EMI/production hardware readiness;
- Dolby certification.

---

## 5. Current merged architecture baseline

Control plane:

```text
Aurora configuration
      |
      v
aurora-config
      |
      v
aurora-runtime-assembly
      |
      +----> aurora-runtime-inspection
      |
      v
aurora-runtime-materialization
      |
      v
prepared renderer + prepared realtime DSP/delay + prepared backend state
      |
      v
aurora-realtime-engine
```

Realtime/media direction conceptually:

```text
source/input adapter -> decoder API -> scene/audio model -> renderer API -> DSP API -> generic realtime audio I/O
```

Application/provider integrations remain separate:

```text
provider/application plugin <-> aurora-plugin-host <-> typed source/control APIs
```

Important stable IDs currently established by runtime assembly:

- `org.aurora.renderer.basic`
- `org.aurora.renderer.vbap`
- `org.aurora.dsp.basic-delay`

Prepared identity in inspection means **selected/prepared intent only**. It must not be phrased as loaded, activated, negotiated, observed, or physically measured unless there is separate evidence.

---

## 6. Repository navigation map

Use this section instead of rediscovering paths.

| Area | Primary paths | Use when changing |
| --- | --- | --- |
| Workspace | `Cargo.toml`, `Cargo.lock` | crate membership, shared deps, MSRV |
| Core capability/layout foundations | `crates/aurora-core/` | layouts, capability truth registry, common foundations |
| Scene/object model | `crates/aurora-scene/` | speaker/object scene representation |
| Renderer contract | `crates/aurora-renderer-api/src/lib.rs` | renderer traits, scratch/output/capability contract |
| Basic renderer | `crates/aurora-renderer-basic/` | Basic/reference implementation |
| VBAP renderer | `crates/aurora-renderer-vbap/` | VBAP implementation |
| Cavern reference adapter | `crates/aurora-renderer-cavern/` | inactive/research external renderer adapter |
| Decoder contract | `crates/aurora-decoder-api/` | decoder boundary |
| Decoder adapters | `crates/aurora-decoder-iamf/`, `crates/aurora-decoder-truehdd/` | research/adapter decoder work |
| DSP contract | `crates/aurora-dsp-api/` | realtime DSP/delay contract |
| Basic DSP | `crates/aurora-dsp-basic/` | basic delay/DSP implementation |
| External DSP | `crates/aurora-dsp-camilladsp/` | CamillaDSP adapter |
| Generic audio I/O | `crates/aurora-audio-io/src/lib.rs` | portable I/O abstractions |
| Realtime backend contract | `crates/aurora-realtime-audio-api/` | backend/device interface |
| CPAL backend | `crates/aurora-realtime-audio-cpal/` | host audio backend |
| Simulated backend | `crates/aurora-realtime-audio-sim/` | deterministic backend tests |
| Realtime engine main path | `crates/aurora-realtime-engine/src/lib.rs` | prepared renderer/DSP engine, callback path |
| Realtime drift/ASRC | `crates/aurora-realtime-engine/src/drift.rs`, `drift_controller.rs`, `asrc.rs` | clock drift/resampling |
| Duplex/transport/device | `crates/aurora-realtime-engine/src/duplex.rs`, `transport.rs`, `device_state.rs` | streaming state/transport/device behavior |
| Latency | `crates/aurora-realtime-engine/src/latency.rs` | latency accounting semantics |
| Realtime acceptance | `crates/aurora-realtime-acceptance/` | acceptance/evidence harness |
| Simulation assurance | `crates/aurora-simulation-assurance/` | scenario campaigns and deterministic assurance |
| Configuration model | `crates/aurora-config/src/model.rs` | root typed config schema |
| Config validation | `crates/aurora-config/src/validation.rs` | fail-closed structural/semantic validation |
| Config migration | `crates/aurora-config/src/migration.rs` | explicit schema migration |
| Config presets | `crates/aurora-config/src/preset.rs` | preset composition/conflicts |
| Config errors/limits | `crates/aurora-config/src/error.rs`, `limits.rs` | structured diagnostics/bounds |
| Config fixtures | `fixtures/config/` | canonical valid/invalid/migration fixtures |
| Config docs | `docs/configuration.md` | public configuration contract |
| Runtime plan/identities | `crates/aurora-runtime-assembly/src/lib.rs` | prepared plan types, component identities, versions |
| Runtime derivation | `crates/aurora-runtime-assembly/src/derivation.rs` | config -> prepared execution plan |
| Runtime setup | `crates/aurora-runtime-assembly/src/setup.rs` | setup/capacity/device preparation |
| Concrete materialization | `crates/aurora-runtime-materialization/src/lib.rs` | build selected concrete renderer/DSP/runtime components |
| Runtime inspection | `crates/aurora-runtime-inspection/` | deterministic JSON/text plan inspection |
| Diagnostics | `crates/aurora-diagnostics/`, `docs/diagnostics.md` | structured truth/diagnostics |
| Source Manager | `crates/aurora-source-runtime/` | source lifecycle/arbitration/transaction boundary |
| Plugin contracts/host | `crates/aurora-plugin-api/`, `crates/aurora-plugin-host/` | application plugin protocol and isolation |
| Plugin architecture | `docs/plugin-architecture.md` | extension/versioning rules |
| Measurement | `crates/aurora-measurement/` | measurement semantics/evidence |
| CLI/tools | `crates/aurora-cli/` | user/control tooling, capability docs/evaluation |
| Immersive/JOC validation | `validation/immersive/` | Harletty/Omniphony/OpenJOC software lanes |
| CI | `.github/workflows/ci.yml` | Linux/Windows/MSRV core CI |
| Realtime soak | `.github/workflows/realtime-health-soak-ci.yml` | sustained realtime health |
| Simulation PR smoke | `.github/workflows/simulation-assurance-pr.yml` | PR deterministic simulation smoke |
| External component policy | `config/external-components-v1.json`, `THIRD_PARTY_LICENSES.md` | pinned external tools/license decisions |

### Fast code-search anchors

Use these symbols before browsing directories manually:

```text
RealTimeEngine::new_with_prepared_components
RendererCapabilities
RealtimeDelayProcessor
PreparedComponentIdentity
PreparedRealtimeComponentSelection
BASIC_RENDERER_IMPLEMENTATION_ID
VBAP_RENDERER_IMPLEMENTATION_ID
BASIC_DELAY_IMPLEMENTATION_ID
INSPECTION_SCHEMA_VERSION
CURRENT_SCHEMA_VERSION
RendererConfiguration
BackendIntent
SourceManager
```

---

## 7. Important completed work / merge landmarks

These are architectural landmarks, not a full changelog.

- **#107** — hardware-agnostic refactor.
- **#108** — paced realtime JOC streaming work.
- **#109–#112** — realtime health acceptance/reporting and sustained accelerated soak foundation.
- **#114** — OpenJOC independent fail-closed differential validation.
- **#117** — modular application/plugin foundation.
- **#120** — atomic plugin package registry/update/rollback.
- **#121** — Source Manager v1 transactional boundary foundation.
- **#125** — moved Basic delay construction out of realtime engine production path.
- **#126** — caller-supplied prepared realtime DSP seam.
- **#127** — renderer materialization decoupled from realtime engine; engine stores generic renderer and uses capabilities.
- **#128** — runtime assembly owns prepared component identities/contract versions; inspection schema v2 reports them; second DSP adapter + failed replacement isolation tests.
- **Issue #118** — closed/completed after #125/#127/#128. Production renderer/DSP coupling acceptance is considered complete.

When citing an old PR as proof, inspect the actual PR/CI rather than relying only on this summary.

---

## 8. Current WIP — Issue #119

Issue: **#119 — Make configuration reference versioned replaceable components instead of hard-coded implementations**.

Goal: replace root configuration implementation enums with versioned `ComponentReference`-style selections resolved by explicit registries, while keeping product intent deterministic, migration-safe, and fail-closed.

### Current slice

Renderer selection first; backend selection follows in a second slice.

WIP branch:

`feature/config-renderer-component-ref-v2`

Last known branch head before the next fix:

`5fdffadd42373da448157c424fa5c6712b1c017e`

No production PR has been opened for this slice yet.

### Intended renderer-slice design already exercised in temporary validation

- root config schema moves from hard-coded `RendererConfiguration::{Basic, PointSourceVbap, HorizontalSpread, ...}` toward a generic versioned component reference;
- component reference carries stable component ID, contract kind/version compatibility, component config schema, and bounded configuration payload;
- schema migration is explicit v1 -> v2 and v0 -> v2;
- current v1 fixtures are retained as migration sources; v2 fixtures become current canonical fixtures;
- runtime assembly owns an explicit renderer component registry;
- Basic and VBAP references resolve through the registry;
- unknown IDs/config schema versions fail closed;
- a test renderer can be added to the registry without adding a new root Aurora configuration enum variant;
- backend enums are intentionally left for the next #119 slice rather than mixing renderer + backend migration in one risky change.

### What validation has already shown

The temporary renderer-v2 runner reached:

- workspace `cargo check` PASS;
- locked workspace `cargo check --locked` PASS;
- migration tests PASS;
- renderer component structural/fail-closed tests PASS;
- config suite reached **16/17 PASS**;
- the remaining failure is only the deterministic canonical fixture checksum test.

### Exact current blocker

Do **not** hash raw fixture JSON to define canonical checksum expectations. `ValidatedConfiguration::canonical_json()` normalizes some values/order, and raw-file hashes differ from canonical hashes for the 5.1 and 7.1 fixtures.

Actual canonical FNV-1a64 hashes reported by the last validation run, in this order:

1. `minimal-v2` -> `0xef8f5bd68119dcf1`
2. `stereo-basic-v2` -> `0x54b72410a4da76ac`
3. `surround-5-1-v2` -> `0xf1eea69ae35151ee`
4. `surround-7-1-v2` -> `0x73890ea81de9a1fb`
5. `irregular-horizontal-v2` -> `0x4db22da621458129`
6. `phase-3a-point-source-v2` -> `0x0fcf1ead7ce3767b`
7. `phase-3b-spread-v2` -> `0x2f2b43d7879cbfe7`

The last runner still had raw-file expectations for #3 and #4, causing the only config-test failure.

### Exact next action

1. Update the renderer-v2 patch/runner so `canonical_fixture_checksums_are_stable` uses the seven canonical values above, not raw JSON hashes.
2. Re-run:
   - format;
   - workspace check;
   - locked workspace check;
   - full clippy with `-D warnings`;
   - config/runtime-assembly/runtime-inspection tests;
   - runtime-materialization/realtime-engine tests.
3. Fix any subsequent real failure; do not commit production changes until all focused validation is green.
4. Commit only production files + `AGENTS.md`; temporary workflows must not enter the final PR.
5. Delete all temporary #119 workflows from the WIP branch before PR. Last known temporary files are:
   - `.github/workflows/temporary-config-v2-inventory.yml`
   - `.github/workflows/temporary-config-v2-renderer.yml`
   - `.github/workflows/temporary-config-v2-renderer-runner.yml`
6. Compare branch against `main-v2`; `.github/workflows/temporary-*` must not remain in the diff.
7. Open focused PR for **renderer component references / config schema v2**.
8. Run official PR gates: CI, Simulation Assurance PR smoke, and Sustained Realtime Health Soak.
9. Merge only after all official gates pass.
10. Update this file to mark renderer slice merged, then continue #119 with the **audio backend component-reference slice** (`Virtual/Cpal/Offline` decoupling).

### #119 is not complete until

- renderer references are merged;
- backend references are merged;
- old configs migrate deterministically;
- registries reject missing/incompatible components before activation;
- runtime-plan inspection reports selected contract + implementation identity/version;
- adding a second renderer/backend does not require a new root config enum;
- full configuration/preset/inspection/simulation/realtime CI is green.

---

## 9. Other open/queued architectural work

- **#115** — evaluate post-v0.5.2 Omniphony changes in an evaluation-only reference lane. Do not replace the stable pin just because newer commits exist.
- **#116** — Source Manager tracker remains open even though Source Manager v1 foundation landed in #121. Reassess acceptance criteria before closing; do not assume the issue is complete solely from #121.
- After #119 control-plane modularity is complete, return priority to the actual immersive critical path: continuous authorized JOC ingestion/render proof and then physical transport/output proof. Do not let plugin/UI work displace the core audio proof path.

---

## 10. Validation commands and gates

Minimum software change gate:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

When lockfile determinism matters:

```bash
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --locked
```

For realtime/performance-sensitive changes, also run the relevant release-mode Criterion/regression and allocation guards already wired into CI.

Immersive software validation entry points:

```bash
bash validation/immersive/test-joc-stack.sh
bash validation/immersive/test-openjoc-reference.sh INPUT_JOC
bash validation/immersive/test-joc-differential.sh
```

Expected JOC stack success markers include:

```text
PLAIN-EAC3-NEGATIVE-CONTROL-PASS
JOC-IEC61937-PASS
7.1.4 render PASS
AURORA JOC SOFTWARE STACK PASS
```

Official PR evidence normally includes:

- `.github/workflows/ci.yml` — Linux stable, Windows stable, Rust 1.78 MSRV and additional deterministic gates;
- `.github/workflows/simulation-assurance-pr.yml` — deterministic scenario smoke + warmed-up allocation guards;
- `.github/workflows/realtime-health-soak-ci.yml` — accelerated sustained realtime health soak.

Never merge a critical realtime/config/runtime refactor just because a temporary workflow passed; official PR gates must also pass.

---

## 11. Temporary workflow policy

Temporary GitHub Actions workflows may be used when no local runner is available, but they are implementation scaffolding only.

Rules:

- prefix them `temporary-`;
- keep them on the feature branch only;
- make them fail before production commit if validation fails;
- never cite their existence as production capability;
- delete them before opening/finalizing the PR;
- verify compare-to-main has no temporary workflow files.

If GitHub Actions cannot push workflow-file changes because of workflow-token permissions, use the GitHub contents API/connector to create/update/delete those files.

---

## 12. Realtime invariants checklist

Before modifying `aurora-realtime-engine`, renderer API, DSP API, audio backend API, drift/ASRC, or callback wiring, check all of these:

- no callback allocation after warmup/preparation;
- no blocking mutex/condvar wait in callback;
- no logging/string formatting in callback;
- no filesystem/process/network control work in callback;
- bounded scratch/capacity validated before activation;
- dynamic renderer delays only used when renderer capabilities say they are meaningful/required;
- component replacement prepares and validates separately before commit;
- callback behavior remains deterministic for equal prepared state/input;
- latency/evidence wording remains truthful;
- existing active runtime survives failed candidate preparation.

---

## 13. Configuration/component versioning invariants

For #119 and later componentization:

- root Aurora schema version and component-specific config schema are separate concepts;
- component ID is stable Aurora identity, not crate/package/repository name;
- contract major/minor compatibility is checked before materialization;
- configuration payload is bounded and validated outside realtime processing;
- provider/platform SDK types never cross into `aurora-config` models;
- unknown IDs and unsupported component-config versions fail closed with structured diagnostics;
- migration from old enums is explicit and deterministic; never silently reinterpret old configs;
- canonical serialization tests are evidence and should not be weakened to make migrations pass.

---

## 14. Update protocol for this file

Every agent that changes Aurora must update this file before declaring the task complete.

### Always update when any of these change

- active issue/branch/PR;
- merged architecture;
- public contract/API;
- config or inspection schema;
- important implementation IDs/versions;
- current blocker/next action;
- proof status or capability claim;
- validation command/gate;
- repository path of an important subsystem;
- temporary workflow state;
- an issue is opened/closed or acceptance meaningfully changes.

### Keep updates compact

Do not paste full PR descriptions, logs, or chat history here. Record only what the next agent needs to act correctly:

```text
State:
Evidence:
Exact blocker:
Files/symbols involved:
Next action:
Do not claim:
```

### Source-of-truth rule

If this file conflicts with current code, CI, an accepted ADR, or a merged PR, the code/CI/ADR/PR wins. Correct this file immediately in the same task.

---

## 15. Original agent rules retained

- Keep Aurora hardware-agnostic. Core crates must not depend on a named phone, SBC, MCU, HDMI/eARC board, DAC, amplifier, speaker product, boot image, or appliance filesystem.
- Put optional platform integrations behind narrow adapter boundaries; they must be removable without changing the core audio model or renderer/DSP APIs.
- Never implement Dolby trademarked or patented codec behavior without explicit legal review.
- Never add DRM circumvention or protected-media extraction code.
- Never copy source from repositories with incompatible licenses.
- Keep third-party decoders, renderers, and DSP engines behind adapters with explicit version/license tracking.
- Update architecture documentation when changing public interfaces.
- Keep accelerated simulation deterministic for equal seeds and allocation-free after scheduler startup.
- Live callbacks must not allocate, block, log, access files/processes, or silently change selected devices.
- Software validation proves software behavior only. Do not turn CI evidence into claims about physical eARC, USB, DAC, amplifiers, speakers, thermals, or wireless links.
- Prefer small, reviewable commits and preserve reproducible validation evidence.
