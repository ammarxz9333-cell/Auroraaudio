# Phase 3B -- Deterministic Horizontal Source Spread And Irregular Layout Support

## Current State

- `execution_state`: `IN_PROGRESS`
- `evaluation_classification`: `none`
- branch: `phase-3b-horizontal-spread`
- branch base: `main-v2` at
  `59e99f1ad2f89fd7fa03658b8a698d5dd9baf5b2`

## Additive API

Phase 3B adds a concrete API to `VbapRenderer`; the Aurora-owned `Renderer`
trait is unchanged. A validated `HorizontalSpread` value is created on the
control thread. Its finite inclusive domain is `0.0..=1.0`, so invalid spread
never reaches the callback path and rejection does not allocate there.

`SpreadRenderObject` pairs the existing compact `RenderObject` with a validated
spread. `render_spread_gains` uses the same configured layout, smoothing
history, output buffer, and scratch allocation as `render_gains`.

## Mathematical Law

Let `p_i` be the accepted Phase 3A point-source gain for speaker `i`, `s` the
validated spread, and `d_i` the shortest horizontal angular distance from the
source to speaker `i` in `[0, pi]`.

At `s = 0`, Phase 3B calls the accepted point-source path without applying any
additional arithmetic. The result is therefore exactly compatible, not merely
approximately compatible.

For `s > 0`, the neighborhood radius is:

```text
r = s * pi
```

A speaker with a valid horizontal direction is spread-eligible when
`d_i <= r + 1e-8`. Its positive distance kernel is:

```text
k_i = 1 / (1 + d_i)
```

Other speakers receive `k_i = 0`. Before blending, duplicate-angle groups share
their combined point-source amplitude equally. Equal sharing prevents input
ordering from selecting one geometrically indistinguishable speaker during
nonzero spread while preserving the accepted configured-order point result at
exactly zero spread.

The unnormalized spread result is:

```text
u_i = (1 - s) * p_i + s * k_i
```

The final gain is power-normalized:

```text
g_i = u_i / sqrt(sum_j(u_j * u_j))
```

If no valid horizontal direction or norm exists, the deterministic fallback is
equal power across configured speakers. Runtime non-finite listener/object state
continues to produce immediate finite silence as documented by Phase 3A.

The participation set is monotonic for a fixed non-degenerate layout: the
point-source pair remains active below maximum spread, the angular neighborhood
only grows as `s` increases, and `s = 1` includes every valid horizontal speaker.

## Determinism And Tie-Breaking

- angular comparisons use shortest wrapped distance and include `-pi/+pi`;
- nonzero-spread equal-span and nearest-direction ties use speaker identifiers,
  with configured index only as a final tie-break for duplicate identifiers;
- exact duplicate directions share their point amplitude equally for nonzero
  spread;
- near-duplicate angles remain distinct;
- exact midpoint gains follow the normalized geometry and stable identifier
  tie rule;
- one speaker receives unit gain; two speakers use the same pair law;
- source/listener coincidence uses the inherited equal-power result;
- output remains configured-channel order, so layout permutations require the
  corresponding output permutation when comparing results.

Repeated calls with identical IEEE-754 inputs, configuration, and history are
bit deterministic on the same supported build target. Test tolerances are
`1e-5` for gain comparison and `1e-4` for unit-power checks.

## Capacity And Safety

No new steady-state container is introduced. Phase 3B transforms the existing
one-float-per-speaker scratch vector in place. Layout and object bounds remain
the checked capacities established during `configure`; output and scratch shape
errors remain structured. Rendering performs no allocation, logging, blocking,
formatting, filesystem, or process access.

Finite calculations use widened intermediates and finite saturation inherited
from Phase 3A. Speaker positions, gain trims, and delays remain setup-validated.
Extreme finite object values cannot propagate NaN or infinity.

## Evidence Method

- geometry, power, safety, compatibility, capacity, and allocation:
  `unit_test`;
- repeatable canonical/irregular sweeps and checksums:
  `deterministic_simulation`;
- Criterion timing and allocation audit observations: `host_api_observation`.

No result in this milestone is a `physical_measurement`. Benchmark timing is not
physical latency.

## Exclusions And Stop Boundary

The exclusions and dependency matrix in `docs/planning/phase-3b-scope.md` are
binding. Stop after the bounded implementation, required tests/fixtures,
allocation audit, benchmarks, complete validation, evaluation record, and an
unmerged Phase 3B pull request. No elevation, HRTF, Ambisonics, room behavior,
hardware work, protected-contract change, or later milestone is authorized.
