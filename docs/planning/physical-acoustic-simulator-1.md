# Physical Acoustic Simulator 1

## Status

- `authorization_state`: `PROPOSED`
- `execution_state`: `NOT_STARTED`
- `evaluation_classification`: `NOT_EVALUATED`
- milestone type: hardware-independent acoustic, endpoint, network, and system simulation
- physical evidence produced: none
- Phase 2 replacement: explicitly no

This document proposes a simulation-first engineering program for Aurora. It does
not authorize implementation by itself. Implementation requires an owner-approved
governance amendment and must proceed through bounded checkpoints.

## Purpose

Aurora shall gain an automated, physically grounded virtual laboratory capable of
exercising the complete software signal path before expensive hardware is purchased.
The laboratory must maximize useful correlation with reality while preserving a
strict evidence boundary: simulated output is never relabeled as measured physical
performance.

The simulator is not a demonstration tool. It is an engineering and decision-support
system intended to:

- discover defects and weak assumptions automatically;
- compare architectures, algorithms, layouts, and parameter sets;
- run deterministic regressions and stochastic stress campaigns;
- quantify uncertainty and model limitations;
- generate machine-readable and human-readable reports;
- convert findings into bounded engineering recommendations;
- reduce the number and cost of physical prototypes.

## Mandatory automation requirement

The simulator shall test the system automatically. A valid campaign must not depend
on a developer manually choosing only favorable scenes or visually interpreting a
single run.

Each campaign shall:

1. generate or load a versioned scenario matrix;
2. execute all applicable scenarios unattended;
3. preserve the seed, configuration, environment, and software revision;
4. calculate objective metrics and confidence/uncertainty fields;
5. classify every result against explicit thresholds;
6. emit detailed reports and a concise engineering summary;
7. identify regressions against an accepted baseline;
8. recommend candidate project changes without applying them automatically;
9. produce exact replay commands for every failure or anomaly;
10. fail the campaign if evidence is missing, mislabeled, or internally inconsistent.

No automatic recommendation may be presented as a validated fix until the proposed
change passes a new campaign and, where applicable, later physical validation.

## Evidence classes

Every metric and conclusion must carry exactly one evidence class:

- `analytical_model`: result derived from documented equations;
- `synthetic_fixture`: result derived from controlled generated signals;
- `virtual_audio_backend`: result observed through Aurora virtual devices;
- `acoustic_simulation`: result derived from room, source, listener, and transducer models;
- `network_simulation`: result derived from packet, clock, loss, and scheduling models;
- `host_api_observation`: host metadata without an accepted physical signal path;
- `physical_measurement`: reserved for accepted Phase 2 or later physical evidence.

Only `physical_measurement` may satisfy a physical acceptance gate.

## Fidelity tiers

Reports shall state the fidelity tier used:

### Tier 0 — Deterministic signal-path truth

- channel order and routing;
- gain, polarity, delay, filtering, crossover, and clipping;
- finite samples and silence preservation;
- deterministic renderer output;
- ASRC coherence and boundedness;
- state transitions and fault propagation.

### Tier 1 — Endpoint and timing models

- independent clocks and clock drift;
- callback jitter and variable callback sizes;
- starvation, bursts, device loss, and recovery;
- buffer occupancy, underflow, overflow, and latency estimation;
- CPU budget and bounded-memory behavior.

### Tier 2 — Networked endpoint models

- latency distributions;
- jitter, packet loss, reordering, duplication, and bursts;
- bounded jitter buffers;
- clock synchronization error;
- recovery after endpoint or network disruption;
- multi-endpoint channel coherence.

### Tier 3 — Linear acoustic room model

- source and listener geometry;
- direct path and propagation delay;
- frequency-dependent air and surface attenuation;
- early reflections;
- late reverberation approximation;
- speaker directivity and frequency response;
- binaural/HRTF preview through headphones;
- impulse-response and spatial-energy reports.

### Tier 4 — Bounded electroacoustic approximation

- amplifier gain and bandwidth;
- modeled noise floor;
- modeled harmonic/intermodulation distortion where supported;
- driver transfer functions;
- crossover interactions;
- enclosure response supplied by validated external data or bounded models.

Tier 4 results must include wide uncertainty bounds unless their parameters originate
from traceable measurements or manufacturer data.

## Scenario matrix

The automated campaign engine shall support combinatorial and constrained-random
coverage across:

- layouts: stereo, 2.1, 5.1, 7.1, 5.1.2, 5.1.4, 7.1.4, irregular layouts;
- room dimensions and aspect ratios;
- listener positions, orientations, and optional head motion;
- speaker placement, orientation, height, and placement error;
- source trajectories, source spread, elevation, and simultaneous objects;
- sample rates, block sizes, channel counts, and signal levels;
- clock drift, callback jitter, device faults, and recovery scripts;
- network latency/loss/jitter distributions and burst conditions;
- speaker profiles and uncertainty ranges;
- wall/floor/ceiling absorption profiles;
- HRTF datasets and listener-profile assumptions;
- nominal, boundary, adversarial, and Monte Carlo cases.

The matrix must be versioned and bounded. Pairwise or higher-order combinatorial
coverage may be used where exhaustive coverage is infeasible, but omitted dimensions
and interactions must be reported.

## Automatic test suites

### A. Correctness and determinism

- identical seed and configuration produce identical checksums;
- scene ordering does not change results;
- silence remains silence;
- no NaN, infinity, denormal storm, or clipping without classification;
- channel masks and canonical order remain correct;
- energy conservation or documented normalization rules hold;
- replay command reproduces each finding.

### B. Timing and real-time safety

- callback deadlines and block budgets;
- p50, p95, p99, and maximum processing time;
- steady-state allocations and memory growth;
- queue occupancy and saturation;
- underflow/overflow frequency;
- recovery time after starvation, burst, and device loss;
- ASRC ratio bounds and controller stability.

### C. Spatial rendering

- angular localization error proxy;
- front/back and elevation ambiguity indicators;
- phantom-center stability;
- source trajectory continuity;
- gain and delay continuity;
- layout symmetry and irregular-layout robustness;
- sweet-spot and off-axis degradation;
- binaural preview consistency across HRTFs.

Spatial metrics are model-based proxies, not claims about human preference or
physical listening quality.

### D. Room acoustics

- direct-to-reverberant ratio;
- early-decay time and reverberation estimates by frequency band;
- speech/music clarity proxies where mathematically valid;
- seat-to-seat response variation;
- modal-risk indicators at low frequencies;
- reflection arrival times and energy;
- robustness to placement and material uncertainty.

### E. Network and multiroom

- endpoint synchronization error;
- inter-channel phase/coherence error;
- latency distribution and boundedness;
- packet recovery behavior;
- jitter-buffer occupancy;
- audible-risk proxies for drops, discontinuities, and resync events;
- long accelerated soak campaigns.

### F. Fault injection

- device disappearance and reappearance;
- invalid configuration;
- corrupt or missing profile data;
- CPU starvation and delayed callbacks;
- packet bursts, loss bursts, duplication, and reordering;
- clock steps and extreme drift;
- partial endpoint failure;
- report-writing interruption and resume behavior.

## Metrics and verdicts

Every metric must define:

- name and unit;
- evidence class and fidelity tier;
- formula or implementation reference;
- expected range;
- warning and failure thresholds;
- uncertainty/confidence field;
- aggregation rule;
- baseline comparison rule;
- whether the metric is eligible for automated recommendation.

Each scenario receives one verdict:

- `PASS`;
- `PASS_WITH_WARNINGS`;
- `FAIL`;
- `INCONCLUSIVE`;
- `MODEL_NOT_APPLICABLE`.

A campaign cannot pass by averaging away a safety-critical failure. Mandatory gates
must use explicit worst-case or percentile criteria.

## Reports

Each campaign shall emit:

### Machine-readable report

A versioned JSON report containing:

- repository revision and dirty-state flag;
- campaign and scenario-matrix versions;
- full configuration and deterministic seeds;
- environment and build profile;
- models, datasets, versions, licenses, and hashes;
- per-scenario metrics and verdicts;
- aggregate statistics and percentile distributions;
- uncertainty and model-validity fields;
- baseline deltas;
- failures, warnings, and exact replay commands;
- generated recommendation records;
- explicit statement that results are simulated.

### Human-readable engineering report

A Markdown or HTML report containing:

- executive summary;
- campaign coverage;
- pass/fail/inconclusive counts;
- highest-risk findings;
- regressions and improvements;
- plots/tables for load, latency, synchronization, spatial, and acoustic metrics;
- model limitations;
- prioritized recommendations;
- requirements for later physical validation.

### CI summary

CI shall publish a concise summary and preserve the detailed report as an artifact.
Required campaign failures must fail CI. Long campaigns may run on a scheduled or
manual workflow but must remain deterministic and replayable locally.

## Recommendation engine

The simulator shall create candidate engineering recommendations from documented,
rule-based mappings. Examples:

- excessive underflow -> inspect buffer target, scheduling budget, or processing load;
- unstable ASRC controller -> inspect gains, bounds, or clock model;
- excessive synchronization error -> inspect clock estimator and correction policy;
- discontinuous trajectory -> inspect renderer interpolation or layout degeneracy;
- poor seat robustness -> inspect layout, delays, gains, or calibration strategy;
- high reflection risk -> inspect speaker direction, room treatment assumptions, or correction scope.

Each recommendation must contain:

- finding identifier;
- supporting metrics and affected scenarios;
- confidence and uncertainty;
- suspected subsystem;
- proposed bounded experiment or code change;
- expected metric movement;
- regression tests required;
- physical-validation requirement;
- status: `PROPOSED`, `UNDER_TEST`, `SUPPORTED_BY_SIMULATION`, or `REJECTED`.

The simulator must never modify production code, thresholds, or accepted baselines
automatically. An engineer or owner must authorize changes.

## Anti-fabrication rules

The following are prohibited:

- selecting only favorable scenarios;
- silently changing seeds, thresholds, or baselines;
- presenting modeled SPL, latency, distortion, localization, or room response as measured;
- using untraceable speaker or room parameters;
- hiding failed or inconclusive scenarios;
- replacing missing data with ideal constants without a warning;
- deriving a physical acceptance claim from HRTF listening alone;
- optimizing against the evaluation set without a separate regression/holdout set;
- accepting a recommendation without rerunning the applicable campaign.

Reports must retain failed, skipped, unsupported, and inconclusive cases.

## Model calibration and later correlation

When physical hardware becomes available, measured datasets may be imported only
through a documented calibration path. The project shall compare simulated and
physical results, estimate model error, and version updated profiles. Historical
reports remain immutable and continue to identify the older model version.

Physical correlation is used to improve the simulator; it does not retroactively
convert earlier simulation into physical evidence.

## Architecture proposal

Implementation should remain modular and may introduce bounded crates such as:

- `aurora-scenario-matrix` — versioned scenario generation and combinatorial coverage;
- `aurora-campaign-runner` — unattended execution, replay, sharding, and resume;
- `aurora-network-model` — deterministic packet and clock impairment models;
- `aurora-acoustic-model` — room, propagation, reflection, and transducer abstractions;
- `aurora-binaural-preview` — optional HRTF rendering with strict license review;
- `aurora-simulation-metrics` — metric definitions, thresholds, and verdicts;
- `aurora-simulation-report` — JSON/Markdown reports and baseline comparison;
- `aurora-recommendation-engine` — rule-based candidate recommendations.

Exact crate boundaries require a separate architecture review. Existing accepted
simulator and renderer contracts must be reused; no duplicate virtual audio backend
may be created.

## Proposed checkpoints

### Checkpoint A — Governance and metric registry

- approve scope and evidence boundaries;
- define report schema;
- define metric registry and verdict semantics;
- define baseline policy and anti-fabrication checks;
- no acoustic implementation yet.

### Checkpoint B — Automated campaign runner

- scenario matrix;
- deterministic sharding and replay;
- unattended execution;
- JSON/Markdown reports;
- baseline/regression comparison;
- CI artifacts and summaries.

### Checkpoint C — Network and endpoint campaign

- deterministic packet and endpoint impairments;
- synchronization, latency, buffer, and recovery metrics;
- accelerated long-duration campaigns;
- recommendation mappings for timing/network findings.

### Checkpoint D — Linear acoustic model

- geometry and propagation;
- speaker profiles and directivity;
- early reflections and bounded reverberation;
- impulse-response generation;
- uncertainty propagation and validation fixtures.

### Checkpoint E — Binaural/HRTF preview

- optional dependency selected after license review;
- headphone preview of supported layouts;
- multiple HRTF profiles;
- head-orientation support where bounded;
- no subjective-quality acceptance claim.

### Checkpoint F — Electroacoustic profiles and design sweeps

- traceable speaker/amplifier profile format;
- bounded noise/distortion/enclosure approximations;
- automatic design-space sweeps;
- Pareto-style reports for quality, latency, load, robustness, and estimated cost inputs.

### Checkpoint G — Simulation-to-physical correlation

- begins only when Phase 2 hardware exists;
- import measured impulse, latency, and endpoint data;
- quantify model error;
- version calibrated profiles;
- preserve strict separation between simulated and measured evidence.

## Acceptance criteria for the proposed program

The program may be considered software-complete only when:

- campaigns execute unattended and are exactly replayable;
- reports contain all mandatory provenance and evidence labels;
- objective thresholds and uncertainty are versioned;
- failures cannot be hidden by aggregation;
- baseline regression detection is verified;
- automated recommendations link to evidence and never alter code automatically;
- nominal, boundary, adversarial, and stochastic campaigns exist;
- report-schema and replay compatibility tests pass;
- performance and memory remain bounded;
- dependency and dataset licenses are documented;
- all outputs state that physical validation remains pending.

## Explicit non-goals

This milestone does not:

- replace Phase 2 physical validation;
- certify a speaker, amplifier, room, or product;
- prove subjective listening quality;
- implement proprietary Dolby/DTS decoding;
- add HDMI/eARC or wireless production transport;
- claim measured SPL, distortion, latency, or localization;
- permit automatic production-code modification;
- permit AI-generated recommendations without a separately approved policy.

## Stop boundary

Agents must stop and request explicit approval before:

- implementation begins;
- a new dependency or HRTF/acoustic dataset is added;
- accepted renderer, transport, ASRC, state-machine, or configuration contracts change;
- thresholds or baselines are relaxed;
- reports are used as physical evidence;
- automatic code modification is introduced;
- Phase 3C, calibration, networking production paths, wireless audio, HDMI/eARC, or proprietary codecs are started.
