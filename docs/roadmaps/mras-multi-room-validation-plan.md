# MRAS Multi-Room Validation Plan

## Decision

Use the Multi-Room Apartments Simulation (MRAS) dataset only as an external validation corpus for Aurora's physical room simulator.

Do not adopt the accompanying neural acoustic-map model, Python training stack, LMDB pipeline, or runtime dependencies.

## Why this is useful

Aurora needs evidence that its simulator behaves plausibly not only inside one shoebox room, but also across connected rooms, doors, corridors, material changes, and moving receivers. MRAS provides exactly this type of synthetic multi-room reference data at scale.

The useful contribution is therefore validation coverage, not implementation code.

## Scope

Curate a compact subset covering:

- same-room propagation;
- one-door adjacent-room propagation;
- multi-room propagation paths;
- narrow and wide openings;
- reflective and absorptive material sets;
- source/receiver distance changes;
- receiver motion through a doorway;
- linear and grid apartment layouts.

## Integration points

- Issue #44: simulation artifacts and objective metrics.
- Issue #52: provenance, licensing, checksums, and dataset admission.
- Issue #55: implementation and acceptance tracking.
- Physical simulator roadmap: generation of Aurora-side RIRs.

## Validation outputs

For each selected case, generate:

- canonical scene conversion report;
- material mapping report;
- Aurora RIR;
- reference MRAS RIR or derived metrics;
- direct-path timing comparison;
- early-reflection comparison;
- RT20/RT30/RT60, EDT, C50, C80, D50, and Ts;
- inter-room attenuation by frequency band;
- doorway-crossing continuity report;
- deterministic checksums;
- pass/fail summary with justified tolerances.

## Data handling

- Route every asset through the #52 registry.
- Record authoritative source, publication, license, attribution, and checksum.
- Avoid full-dataset vendoring.
- Prefer download-time acquisition or external CI cache.
- Keep a tiny metric-only smoke set in normal CI.
- Keep waveform comparison in scheduled or manual assurance runs.

## Explicit exclusions

- neural scene-wide acoustic estimation;
- model training or inference;
- PyTorch, Conda, or LMDB dependencies in Aurora;
- shipping millions of RIRs;
- treating synthetic MRAS results as the only validation source;
- demanding exact waveform identity between different simulation methods.

## Completion criteria

- At least 12 curated cases.
- Coordinate and unit conversion verified.
- Material approximations exposed explicitly.
- Same-room and cross-room fixtures automated.
- Doorway transition continuity automated.
- Metric tolerances justified.
- CI smoke tier and extended waveform tier separated.
- All provenance and attribution requirements satisfied.
