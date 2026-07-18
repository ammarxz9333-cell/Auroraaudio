# Aurora Declarative Audio Graph Extraction Plan

## Status

- Decision: **adapt selected Elementary architecture ideas; do not adopt Elementary as Aurora's runtime**.
- Tracking issue: `#51`.
- Reference project: `elemaudio/elementary`.
- License of the reference project: MIT; copied or closely derived code still requires attribution and review.
- Product timing: implement only after a concrete dynamic-routing use case exists, but before production multiroom, runtime device replacement, or complex DSP-chain reconfiguration.

## Objective

Create an Aurora-owned Rust control-plane graph compiler that can describe, validate, reconcile, and atomically publish audio-processing configurations without rebuilding the complete realtime engine for every parameter or routing change.

This plan does not create a visual modular synthesizer, JavaScript runtime, plugin host, or second scene model.

## 1. Extracted architectural ideas

Aurora adopts the following concepts:

1. declarative desired graph separate from the executing runtime;
2. stable node identity derived from explicit IDs and structural content;
3. differential reconciliation between desired and active graphs;
4. separation of topology changes from parameter-only changes;
5. batched update plans followed by one explicit commit;
6. portable graph description independent from audio-device and transport backends;
7. optional root crossfade when replacing an executing graph.

Aurora rejects:

- JavaScript as a required shipped control layer;
- C++ runtime integration as the default engine;
- allocation, graph traversal, hashing, map access, or reconciliation inside the audio callback;
- hash-only correctness without collision protection;
- a general-purpose modular-synthesis scope.

## 2. Aurora-owned graph model

Define a small immutable typed graph representation containing only control-plane data.

Initial node categories:

- input/source;
- decoder or PCM ingestion boundary;
- renderer;
- DSP processor;
- mixer;
- channel router or mapper;
- delay/trim;
- network sender or receiver boundary;
- device/output sink;
- room/group routing root.

Every node must declare:

- stable logical node ID;
- node kind and version;
- typed input and output ports;
- supported channel counts or layouts;
- topology-affecting properties;
- realtime parameter set;
- required capabilities and resources;
- state-preservation policy.

The graph description must not contain live device handles, sockets, locks, file readers, native pointers, callback closures, or mutable DSP state.

## 3. Identity model

Aurora must separate three identities:

### Logical identity

A user- or system-assigned stable ID representing the intended persistent component.

### Structural identity

A canonical structural digest including:

- node kind and schema version;
- topology-affecting properties;
- input node identities;
- selected output port/channel;
- immutable external-resource identity where applicable.

### Runtime generation identity

A monotonically increasing graph generation used for publication, diagnostics, and retirement.

A structural hash is an optimization only. Equality must verify canonical data or an equivalent collision-safe key.

Changing gain, mute, source position, delay target, or other smooth parameters should not normally change topology identity.

## 4. Validation and compilation

Compile the desired graph outside realtime.

Validation must reject:

- cycles unless a specifically supported feedback node defines bounded delay;
- missing or duplicate logical IDs;
- nonexistent ports;
- incompatible sample formats;
- incompatible channel counts or layouts;
- unsupported renderer/backend capabilities;
- invalid parameter ranges;
- unbounded buffers or resource requirements;
- unavailable external resources;
- graphs exceeding configured CPU or memory budgets when estimates are available.

Compilation produces an immutable runtime snapshot containing:

- topologically ordered execution schedule;
- preallocated node state;
- preallocated audio buffers;
- routing tables;
- parameter handles or indices;
- latency accumulation data;
- capability and resource manifest;
- generation ID.

## 5. Differential reconciliation

Compare the desired graph against the active logical graph and classify every change as:

- unchanged;
- parameter-only update;
- reusable node with changed routing;
- node replacement;
- new node;
- retired node;
- root activation or retirement.

The reconciler emits a deterministic bounded plan containing operations such as:

- create and initialize node off-thread;
- reuse stateful node;
- prepare parameter update;
- connect or disconnect edge in the next snapshot;
- activate new roots;
- retire unreachable nodes after a safe generation boundary.

Identical graph submissions must produce no runtime rebuild. Parameter-only updates must preserve state unless the node explicitly declares otherwise.

## 6. Transactional publication

Graph mutation follows this sequence:

1. construct desired graph;
2. validate and reconcile off-thread;
3. allocate and initialize all new resources;
4. build complete immutable generation `N+1`;
5. queue publication through a bounded control path;
6. swap generation only at a block boundary;
7. optionally run old and new roots concurrently for a bounded crossfade;
8. retire generation `N` outside the callback after no realtime reference remains.

The callback must observe either complete generation `N` or complete generation `N+1`, never a partially mutated graph.

No node construction, destruction, allocation, lock acquisition, graph traversal, hashing, or logging formatting may occur in the callback.

## 7. Transition policy

Support three transition classes:

### Parameter smoothing

For topology-stable changes such as gain, position, delay target, and filter parameters.

### Immediate generation swap

For silent, stopped, or explicitly discontinuous workflows.

### Root crossfade

For audible topology changes, device replacement, renderer replacement, or routing changes where a direct swap creates discontinuity.

Evaluate:

- equal-power versus linear fades;
- configurable duration and upper bound;
- CPU overlap during two-generation execution;
- peak-memory overlap;
- discontinuity energy;
- transition latency;
- state-tail handling for reverberation, convolution, and delays.

## 8. First product use cases

Do not implement the graph system without a concrete consumer. The first accepted implementation should cover at least two of:

1. replace a renderer or DSP chain without stopping the process;
2. change loudspeaker layout or device routing safely;
3. reconnect an output device through an atomic graph replacement;
4. add or remove a multiroom receiver/room root;
5. reroute a source between theater and multiroom operating modes;
6. insert or bypass CamillaDSP while preserving the Aurora-owned control boundary.

## 9. Testing and evidence

Required deterministic tests:

- identical desired graph produces an empty update plan;
- parameter-only change preserves node identity and state;
- topology change generates deterministic operations;
- output-port/channel selection participates in structural identity;
- collision-safe identity behavior;
- invalid cycles and ports fail before publication;
- incompatible channel layouts fail before publication;
- unsupported backend capabilities fail during compile;
- callback sees only complete generations;
- generation retirement does not happen in the callback;
- bounded queue overflow policy is explicit and tested;
- root transition tests produce WAV and discontinuity artifacts through issue `#44`.

Required benchmarks:

- graph validation and compilation time;
- reconciliation time for parameter-only and topology changes;
- peak control-plane memory;
- generation-swap callback overhead;
- root-crossfade CPU and memory overlap.

## 10. Integration order

1. complete `#43A`;
2. complete common evidence in `#44`;
3. complete `#48`, `#45`, `#50`, and `#38`;
4. identify a minimum dynamic-routing consumer from DSP, receiver, or multiroom work;
5. implement issue `#51` as small separate PRs;
6. require the accepted graph publication model before production multiroom routing and device-reconnect behavior.

Issue `#51` must not delay the current renderer critical path and must not be bundled with HRTF, IAMF, packet transport, or complete multiroom implementation.

## Completion rule

This capability is accepted only when Aurora has:

- a minimal immutable graph schema;
- validator and compiler;
- collision-safe identity model;
- differential update planner;
- atomic generation publication prototype;
- documented state-preservation rules;
- root transition evidence;
- realtime no-allocation/no-lock evidence;
- benchmarks and CI artifacts;
- explicit adopted/modified/rejected decision record for Elementary-inspired concepts.