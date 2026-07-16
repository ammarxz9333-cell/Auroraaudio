# Legacy Repository Review

## Review Boundary

This review compares the documentation-only history preserved on `main` and
`legacy/pre-master-reference` with the canonical implementation on `main-v2`.
The histories remain unrelated and were inspected with `git show`; no legacy
branch was checked out or merged.

`AURORA_MASTER_REFERENCE.md` remains authoritative. This document records useful
context but does not approve architecture, networking, hardware, or milestone
work.

Reviewed legacy material:

- `AURORA_MASTER_BLUEPRINT.md`
- `PROJECT_STATUS.md`
- `docs/planning/TASK_REGISTRY.md`
- `docs/architecture/AWAT_SPECIFICATION.md`
- `docs/architecture/AWAT_DIAGRAMS.md`
- `docs/project/PROJECT_DECISIONS.md`
- `docs/project/PROJECT_VISION.md`
- `docs/agent/AGENT_CONTINUATION_PROTOCOL.md`

## Already Represented

| Legacy item | Canonical representation |
| --- | --- |
| Simulation before hardware | Master Sections 15-17 and accepted Simulation Sprint 1 record |
| Realtime correctness before features | Master callback invariants and `AGENTS.md` |
| Bounded queues and fixed callback resources | Realtime engine, SPSC transport, allocation tests, Master Sections 8 and 13 |
| No silent fallback or device switching | Device selector and state-machine contracts |
| Simulation is not physical measurement | Master honesty rules, ADR 0011, simulation reports |
| Independent clocks and continuous ASRC | Realtime engine, Rubato adapter, PI controller, simulator |
| Canonical channel roles and explicit routing | Core/scene models and routing validation |
| Backend and third-party isolation | Aurora-owned traits and adapter crates |
| Deterministic fault simulation | `aurora-realtime-audio-sim` and fault fixtures |
| Explicit hardware evidence requirements | Master Phase 2 and physical latency documentation |
| Cold-start and checkpoint discipline | Master required workflow and standard report template |

These items need no import from the legacy history.

## Obsolete

- `PROJECT_STATUS.md` says source synchronization and Simulation Sprint 1
  acceptance are pending. Both are now complete on `main-v2`.
- The legacy repository declares `main` authoritative. GitHub now uses
  `main-v2`; `main` is retained only as an archive.
- `AURORA_MASTER_BLUEPRINT.md` claims sole authority. The accepted canonical
  authority is `AURORA_MASTER_REFERENCE.md`.
- Legacy tasks `REPO-001`, `REPO-002`, and `SIM1-ACC-001` describe work already
  resolved by the dual-history integration and formal acceptance record.
- Reported simulation results in the legacy status were explicitly unverified;
  the canonical acceptance independently verified and froze them.
- Instructions to reconcile the histories through a merge are superseded by the
  approved dual-history branch strategy.

## Conflicting

- The legacy product mission prioritizes synchronized multiroom and wireless
  rear speakers. The master currently defines a hardware-independent spatial
  platform and schedules wireless endpoints only in Phase 7.
- The legacy sequence starts Graph Compiler contracts and AWAT immediately after
  simulation. The master authorizes Phase 2 physical hardware validation next
  and explicitly delays networking and wireless work.
- The legacy blueprint names Raspberry Pi as the primary deployment target. The
  master prohibits hard-coding Aurora to one hardware vendor and treats embedded
  hardware as later research.
- Legacy latency ceilings of 25-30 ms end-to-end, 12 ms AWAT, 8 ms DSP, and 5 ms
  decoder are provisional design targets. They are not accepted measured values
  and cannot replace the canonical latency terminology or evidence rules.
- The legacy blueprint declares broad Sections 4-11 architecture frozen, but
  those contracts were never accepted into the implementation history. They
  cannot override current traits, buffer ownership, transport, ASRC, state
  machine, or fault semantics.
- The AWAT documents propose networking, authentication, packetization, and
  wireless behavior that is outside the currently authorized milestone.

No conflicting item was imported.

## Candidate For Future Milestone

The following ideas are technically relevant but remain proposals until the
master schedules them:

- immutable compiled processing plans with explicit admission checks;
- versioned processing epochs for coherent control updates;
- static latency and scratch-memory planning;
- bounded one-way observability that cannot backpressure audio;
- capability manifests independent of operating-system names;
- explicit timestamp, clock-domain, stream-epoch, and sample-index types;
- hierarchical session latency budgeting;
- a structured architecture validation suite;
- Ethernet validation before adding wireless variability;
- deadline-aware packet handling, reorder windows, and bounded jitter buffers;
- protocol fuzz/property tests and deterministic virtual-network faults;
- explicit security threat modeling before any non-lab network transport;
- grouped multichannel endpoint evaluation as a possible future cost/complexity
  tradeoff.

These candidates must be evaluated against existing Aurora APIs and accepted by
the appropriate future milestone. They are not implementation backlog for Phase
2.

## Requires Owner Decision

- Whether Aurora's product mission should prioritize wireless rear surround and
  multiroom over the current broader hardware-independent platform direction.
- Whether an immutable Graph Compiler becomes a future core contract; adopting
  it would affect public APIs, buffer ownership, and realtime lifecycle.
- Whether AWAT should exist as an Aurora-owned protocol, and in which Phase 7
  checkpoint protocol/security design would be authorized.
- Whether Raspberry Pi should be a tested reference target without becoming a
  permanent architectural dependency.
- Whether Ethernet proof must gate all later wireless validation.
- Whether room timelines, graph epochs, and capability manifests merit separate
  ADRs after physical backend validation.
- Which latency targets are appropriate after physical measurements exist.

Each decision requires an explicit master-reference update or accepted ADR
before implementation.

## Conclusion

The legacy history contains useful future design research, especially around
bounded graph planning, validation, timing, and eventual network transport. It
also contains obsolete status and a milestone order that conflicts with the
accepted master reference. The content remains preserved on the legacy branches;
none of it was merged, silently adopted, or implemented during this review.
