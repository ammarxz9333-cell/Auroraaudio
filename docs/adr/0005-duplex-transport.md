# ADR 0005: Contiguous SPSC Frame Ring for Duplex Transport

- Status: Accepted for pre-live validation
- Date: 2026-07-16

## Context

Aurora must cross independent input/output callback threads with bounded memory,
zero steady-state allocation, no locks, coherent multichannel frames, variable
host callback tails, and future drift compensation. The initial
`ArrayQueue<f32>` performs queue synchronization for every sample.

## Decision

Use an Aurora-owned contiguous interleaved SPSC frame ring. The producer and
consumer publish monotonic sample indices once per block in the normal path.
Frame-level peek/discard operations are available only for bounded correction
callbacks. Keep the sample queue and fixed block pool as benchmark prototypes,
not runtime defaults.

## Rationale

The fixed block pool benchmark is fastest, but fixed slot shapes complicate host
callback tails and require partial-block ownership or another copy for drift
transitions. The contiguous ring is substantially faster than the sample queue,
uses a minimum six uncontended atomic operations per normal round trip, and
supports whole-frame corrections without allocating or exposing third-party
types. Its measured threaded p95 remained below 0.1% of block budget in every
tested selected-transport case.

## Consequences

The unsafe SPSC storage relies on exactly one producer and one consumer. Tests
cover bounds, ordering, interleaving, allocation, and threaded shutdown, but Rust
tests do not prove memory ordering on every CPU. Live CPAL integration remains
blocked until this checkpoint is reviewed. Sample-slip crossfade remains a
proof-of-concept strategy and must not be presented as production ASRC.

