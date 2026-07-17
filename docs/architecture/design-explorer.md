# Automatic Design Explorer

## Status

- `authorization_state`: `PROPOSED`
- `execution_state`: `NOT_STARTED`

## Purpose

The Automatic Design Explorer shall search bounded Aurora design spaces and rank candidate configurations using explicit engineering objectives, constraints, uncertainty, and reproducible evidence.

It is a decision-support system, not an autonomous product designer and not an authority to change production code or hardware plans.

## Search dimensions

Supported dimensions may include:

- room dimensions and acoustic-surface profiles;
- speaker count, layout, position, orientation, and placement tolerance;
- listener positions and coverage zones;
- speaker, amplifier, DAC, and endpoint profiles;
- crossover frequencies and slopes;
- delays, trims, routing, and correction parameters;
- buffer targets, synchronization policies, and ASRC controller parameters;
- network assumptions and endpoint roles;
- power, compute, memory, bandwidth, and cost estimates.

Every parameter requires allowed ranges, units, provenance, and uncertainty bounds.

## Objectives and constraints

Candidate ranking may consider:

- spatial-rendering error proxies;
- seat-to-seat response consistency;
- synchronization and latency margins;
- robustness to placement, clock, and network uncertainty;
- CPU, memory, bandwidth, and power budgets;
- modeled component cost;
- implementation complexity and validation burden.

Hard constraints must never be hidden inside a weighted score. Safety, real-time, compatibility, licensing, and mandatory acceptance gates remain explicit pass/fail constraints.

## Search process

1. validate a versioned design-space definition;
2. generate deterministic initial candidates;
3. evaluate candidates through the Validation Lab;
4. retain complete results, including failures;
5. refine the search using documented algorithms;
6. evaluate finalists against a separate holdout matrix;
7. report Pareto-optimal candidates rather than claiming a single universal optimum;
8. produce exact replay information and sensitivity analysis.

## Required report content

- design-space version and seed;
- evaluated and rejected candidate counts;
- objectives, constraints, and weighting policy;
- Pareto frontier and top candidates;
- metric values, uncertainty, and baseline deltas;
- sensitivity to parameter changes;
- robustness under adverse scenarios;
- estimated cost and power assumptions with provenance;
- unresolved risks and physical measurements required.

## Anti-overfitting rules

- evaluation and holdout scenario matrices must be separate;
- thresholds and weights may not be changed silently after results are observed;
- missing component data must not be replaced by ideal values without warnings;
- simulated ranking must not be presented as confirmed listening preference;
- hardware selection remains provisional until physical correlation and acceptance.
