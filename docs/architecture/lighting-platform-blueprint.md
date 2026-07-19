# Aurora Lighting Platform Blueprint

Status: Future architecture blueprint. Documentation only. Implementation is explicitly deferred until the Aurora audio system is complete and accepted.

## 1. Decision

Aurora Lighting is a future peer subsystem of Aurora Audio. It is not an audio feature, not an audio plugin, and not part of the realtime audio core.

Aurora Audio and Aurora Lighting may run on the same Raspberry Pi for cost and simplicity, but they remain separately owned, separately supervised services with independent contracts, lifecycle, persistence, failure handling, testing, and release gates.

No lighting implementation work may begin merely because this blueprint exists. A later governance decision must explicitly activate the lighting roadmap after the audio completion gate defined below has been met.

## 2. Product intent

The future lighting system may provide:

- centralized control of whole-home lighting through the Aurora application;
- homes, rooms, zones, groups, devices, scenes and automations;
- static, scheduled, cinema-reactive and music-reactive lighting;
- compatible third-party lighting through isolated adapters;
- first-party Aurora light strips, light bars, controllers and future lighting hardware;
- same-device and separate-device deployment without changing the domain model.

The first implementation target is lighting. General-purpose home automation, plugs, curtains, HVAC, security and unrelated appliance control are outside this blueprint unless approved by later architecture decisions.

## 3. Platform architecture

```text
                           Aurora Mobile / Desktop App
                                      |
                                      v
                            Aurora Control Plane
                  authentication | routing | service registry
                                      |
                +---------------------+----------------------+
                |                                            |
                v                                            v
      Aurora Audio Service                        Aurora Lighting Service
                |                                            |
       Aurora Audio Core                         Aurora Lighting Core
                |                                            |
   renderer / DSP / transport          rooms / devices / scenes / effects
                |                                            |
          audio drivers                              lighting drivers
                |                                            |
            speakers                       strips / bars / lamps / nodes
```

The control plane provides a shared application surface but contains neither DSP logic nor lighting effect logic.

## 4. Mandatory isolation rules

The following dependencies are permanently forbidden:

```text
Aurora Audio Core      -> Aurora Lighting Core
Aurora Lighting Core   -> renderer internals
Aurora Lighting Core   -> DSP internals
Lighting driver        -> audio callback
Audio driver           -> lighting device implementation
Application UI         -> physical lighting protocol directly
Third-party adapter    -> core domain model mutation
```

The only permitted audio-to-lighting path is a versioned Aurora-owned event or telemetry contract outside the realtime path.

Lighting failures, overload, restarts, corrupted configuration, unavailable devices or network loss must never interrupt audio playback.

## 5. Planned component model

```text
Aurora Lighting Service
|
+-- Lighting API boundary
+-- Lighting Core
|   +-- Site / Home Manager
|   +-- Room and Zone Manager
|   +-- Device Registry
|   +-- Capability Engine
|   +-- Desired-State Engine
|   +-- Group Manager
|   +-- Scene Engine
|   +-- Effect Engine
|   +-- Automation Engine
|   +-- Reactive Lighting Coordinator
|   +-- Health and Diagnostics
|   +-- Persistence boundary
|
+-- Device Manager
|   +-- Discovery
|   +-- Pairing
|   +-- Authentication
|   +-- Authorization
|   +-- Connection supervision
|   +-- Reconnection
|   +-- Firmware compatibility
|   +-- Update orchestration
|
+-- Driver Host
|   +-- Generic addressable LED driver
|   +-- Network lighting driver
|   +-- Future Aurora Device Protocol driver
|   +-- Optional third-party interoperability adapters
|
+-- Runtime boundary
    +-- command queues
    +-- event queues
    +-- timers
    +-- resource budgets
    +-- process supervision
```

Component names are provisional. Ownership and dependency direction are normative.

## 6. Domain model

The lighting core should model the following entities independently of any vendor protocol:

### Site

A complete Aurora installation, normally one home.

### Room

A user-facing physical room such as cinema, living room, bedroom or garden.

### Zone

A controllable lighting area within a room, such as TV wall, ceiling, left wall or cabinet.

### Device

A physical or virtual lighting endpoint with stable identity, firmware metadata, transport metadata, health and capabilities.

### Capability

A versioned declaration of what a device can do. Candidate capabilities include:

- power;
- brightness;
- RGB;
- RGBW;
- tunable white;
- segmented or pixel-addressable colour;
- transition duration;
- local effects;
- device-side scene storage;
- power, voltage and temperature telemetry;
- firmware update support.

### Group

A logical collection of devices controlled together while preserving individual device identity.

### Scene

A named desired-state snapshot or policy such as Movie, Reading, Sleep, Music or Party.

### Effect

A time-varying lighting algorithm such as static colour, gradient, ambient video mapping, pulse or music reaction.

### Automation

A trigger-condition-action rule that belongs to the lighting subsystem.

### Desired state and reported state

The core must distinguish user intent from device-reported reality. Drivers reconcile desired state with reported state and expose explicit degraded or unavailable status.

## 7. Application and control flow

```text
User action
    |
    v
Aurora App
    |
    v
Aurora Control Plane
    |
    v
Lighting command contract
    |
    v
Aurora Lighting Service
    |
    +--> authorization
    +--> domain validation
    +--> desired-state update
    +--> scene / effect resolution
    +--> driver command
    |
    v
Physical lighting device
    |
    v
Reported state / health
    |
    v
Lighting Service -> Control Plane -> App
```

The application must not embed device-specific behaviour. Unsupported capabilities are hidden or reported through the capability model.

## 8. Scene and effect architecture

The Scene Engine owns declarative desired states. The Effect Engine owns time-varying output generation. Drivers only translate normalized commands into hardware-specific operations.

```text
Scene selection
      |
      v
Scene Engine
      |
      +--> static desired state
      |
      +--> activates Effect Engine
                       |
                       v
               normalized light frames
                       |
                       v
                    Driver API
```

A scene may reference an effect but must not contain driver-specific commands.

## 9. Audio-reactive lighting boundary

Audio-reactive lighting remains a lighting feature consuming audio-published telemetry.

```text
Realtime audio callback
        |
        v
bounded non-blocking telemetry handoff
        |
        v
non-realtime audio telemetry worker
        |
        v
versioned event contract
        |
        v
Lighting Reactive Coordinator
        |
        v
Effect Engine
        |
        v
Lighting Driver API
```

Candidate telemetry includes:

- playback state;
- media timeline position;
- channel-energy summaries;
- bounded spectral-band summaries;
- beat or transient markers;
- cinema or music mode state;
- explicit scene-selection events.

Every synchronization-sensitive event must include schema version, monotonic timestamp, sequence number and source identity.

Queues must be bounded. Stale reactive events may be dropped or coalesced. Backpressure must never reach the audio callback.

Raw PCM transfer is not approved by this blueprint. Any later raw-audio analysis path requires a separate ADR, resource budget, privacy review and realtime-isolation proof.

## 10. Driver architecture

```text
Aurora Lighting Core
        |
        v
Aurora-owned Lighting Driver API
        |
        +-- Addressable LED adapter
        +-- LAN lighting adapter
        +-- Future Aurora controller adapter
        +-- Optional vendor adapter
```

Drivers own:

- transport details;
- protocol serialization;
- device discovery integration;
- device-specific limits;
- retries and timeouts;
- capability translation;
- reported-state polling or subscription;
- firmware-specific compatibility handling.

Drivers do not own rooms, groups, scenes, automations or application policy.

No initial architecture decision binds Aurora to Govee, Philips Hue, WS2812, SK6812, Raspberry Pi GPIO or a specific wireless protocol.

## 11. Future Aurora Device Protocol

A future Aurora Device Protocol may be introduced for first-party hardware and compatible devices. It must remain separate from the application API and from audio transport.

Candidate protocol responsibilities:

- discovery;
- secure pairing;
- device identity;
- mutual authentication;
- capability advertisement;
- command and state exchange;
- health reporting;
- protocol and firmware version negotiation;
- secure firmware updates;
- revocation and factory reset.

Candidate message envelope fields:

```text
protocol_version
device_id
session_id
message_id
sequence_number
timestamp
message_type
capability_namespace
payload
integrity/authentication data
```

The final transport and cryptographic design require a later security review and ADR.

## 12. Security boundary

Before physical device implementation, the design must define:

- local ownership and pairing ceremony;
- authenticated device identity;
- per-device authorization;
- credential storage;
- device removal and revocation;
- replay protection;
- encrypted control where required;
- signed firmware and rollback policy;
- safe factory reset;
- local-only operation and optional cloud boundaries.

Cloud connectivity is not required by this blueprint. Local control must remain a first-class deployment model.

## 13. Deployment modes

### Same-host mode

```text
Raspberry Pi / supported host
|
+-- Aurora Control Plane
+-- Aurora Audio Service
+-- Aurora Lighting Service
+-- local lighting driver host
```

Requirements:

- separate service lifecycle;
- separate configuration domains;
- bounded IPC;
- audio scheduling priority;
- explicit CPU, memory, I/O and network budgets;
- lighting degradation before audio deadline risk;
- independent restart and health reporting.

### Separate-host mode

```text
Audio host                               Lighting host
+-- Control Plane endpoint  <-------->   +-- Lighting Service
+-- Audio Service                       +-- Driver Host
+-- Audio Core                          +-- Local device links
```

Moving lighting to another host must be configuration-only from the audio core's perspective.

### Future first-party controller mode

```text
Aurora main unit
      |
      v
Aurora Device Protocol
      |
      v
Aurora Lighting Controller
      |
      +-- light strips
      +-- light bars
      +-- room nodes
```

First-party devices use the same capability model and public driver contracts as supported third-party devices. Product-specific features use versioned optional capabilities rather than private core hooks.

## 14. Persistence and configuration

Audio and lighting configuration must be physically and logically separable.

Lighting persistence may include:

- site and room topology;
- device identities and pairing metadata;
- zone and group membership;
- capability snapshots;
- scenes and automations;
- desired state;
- user preferences;
- driver configuration;
- migration version.

Corruption or migration failure in lighting persistence must not prevent the audio service from loading its own configuration.

Configuration schemas must be versioned and migration-tested.

## 15. Resource and failure policy

Required outcomes:

- Lighting process crash: audio continues.
- Driver crash or hang: driver is isolated, timed out or restarted.
- Device unavailable: explicit degraded state; no global stall.
- Network loss: local audio continues; lighting reconnects independently.
- Event backlog: stale reactive events are dropped or coalesced.
- CPU pressure: effect frame rate or complexity is reduced before audio is threatened.
- Memory pressure: bounded caches and queues prevent unbounded growth.
- Configuration failure: lighting enters safe degraded mode without affecting audio.
- Firmware incompatibility: device is quarantined or limited to compatible capabilities.

## 16. Observability

Lighting diagnostics should be separate from audio diagnostics while the control plane may aggregate health.

Candidate lighting metrics:

- service uptime and restart count;
- command latency;
- device acknowledgement latency;
- event queue depth and drops;
- effect frame rate;
- CPU and memory budget use;
- discovery and reconnect attempts;
- device availability;
- driver timeout and error counts;
- protocol and firmware compatibility state.

Lighting logs must not run in the audio callback or share an unbounded logging path with realtime audio.

## 17. Testing strategy

Future implementation should require:

### Unit tests

Domain entities, capability validation, desired-state reconciliation, scene resolution, automation logic and protocol serialization.

### Contract tests

App-to-control-plane, control-plane-to-lighting-service, event schemas and driver API compatibility.

### Simulation tests

Virtual homes, rooms, zones, devices, latency, packet loss, disconnections, stale state, firmware mismatch and large installations.

### Fault-injection tests

Driver hangs, lighting process crashes, corrupted state, queue saturation, network partition and CPU pressure.

### Same-device assurance

Long-duration audio playback while lighting effects, discovery, reconnects and failures are exercised. Acceptance requires no audio underruns attributable to lighting.

### Physical validation

Supported controllers, strips, lamps and first-party prototypes tested with documented firmware and topology.

Audio and lighting acceptance reports remain separate.

## 18. Planned repository boundaries

Illustrative future layout:

```text
crates/
+-- aurora-audio-core
+-- aurora-audio-runtime
+-- aurora-control-plane
+-- aurora-event-contracts
+-- aurora-lighting-contracts
+-- aurora-lighting-core
+-- aurora-lighting-runtime
+-- aurora-lighting-driver-api
+-- aurora-lighting-sim
+-- aurora-device-protocol

services/
+-- aurora-audio-service
+-- aurora-lighting-service

adapters/
+-- lighting-addressable-led
+-- lighting-network
+-- lighting-aurora-device
+-- lighting-third-party
```

Names may change through later ADRs. Audio crates must not depend on lighting crates.

## 19. Deferred implementation phases

These phases are planning placeholders, not active work.

### L0 - Architecture activation

Confirm audio completion gate, review this blueprint, define ADRs, threat model, resource budgets and minimum product scope.

### L1 - Lighting domain simulation

Implement capability-based domain model, scenes, desired state and virtual devices without physical drivers.

### L2 - Independent lighting service

Add service lifecycle, persistence, API contracts, diagnostics and simulated driver host.

### L3 - First physical lighting vertical slice

One supported controller or addressable strip, one room, static colour, brightness, health and restart proof.

### L4 - Central home-lighting management

Rooms, zones, groups, scenes, discovery, pairing and application integration.

### L5 - Reactive lighting

Versioned audio telemetry consumption, bounded queues, effect engine and synchronization validation.

### L6 - Distributed lighting nodes

Separate-host deployment, node discovery, security and network fault handling.

### L7 - First-party Aurora hardware

Aurora Device Protocol, secure firmware lifecycle and supported Aurora lighting products.

Every phase requires an isolated PR series and explicit acceptance evidence.

## 20. Audio completion gate

Lighting implementation must remain deferred until project governance explicitly confirms that the planned Aurora audio system is complete enough to stop treating lighting as a distraction.

At minimum, the activation decision should require:

- accepted local physical-output vertical slice;
- accepted runtime lifecycle and callback safety;
- stable renderer and DSP ownership boundaries;
- accepted simulation assurance for the current audio scope;
- documented supported hardware and known limitations;
- stable configuration, diagnostics and recovery paths;
- accepted multiroom scope defined by the audio roadmap;
- no unresolved architecture blocker that lighting work would bypass;
- explicit governance approval to start Lighting L0.

Until that gate is approved:

- no lighting crate;
- no lighting dependency;
- no lighting daemon;
- no GPIO implementation;
- no lighting UI;
- no device protocol implementation;
- no reactive analyzer;
- no hardware purchasing requirement;
- no change to the audio roadmap priority.

Documentation maintenance and correction of this blueprint are permitted.

## 21. Future acceptance criteria

A future lighting release is acceptable only when it proves:

- audio builds, tests and runs with lighting absent;
- lighting runs on the same host without entering the realtime path;
- lighting can move to a separate host by configuration;
- lighting restart does not interrupt audio;
- all cross-service messages are Aurora-owned and versioned;
- all queues and caches are bounded;
- backpressure cannot reach the audio callback;
- device-specific logic remains behind drivers;
- scenes and effects are independent of hardware protocol;
- local control works without mandatory cloud service;
- security and firmware boundaries are documented and tested;
- simulation and physical validation reports are separate and reproducible.

## 22. Non-goals of this blueprint

This blueprint does not:

- implement lighting;
- alter the current audio architecture;
- change the audio roadmap order;
- select final hardware;
- guarantee compatibility with Govee or another vendor;
- authorize proprietary protocol reverse engineering;
- introduce cloud infrastructure;
- turn Aurora into an unrestricted home-automation platform;
- claim a consumer-ready lighting product.

## 23. Governance rule

Any future agent, developer or contributor working on Aurora Lighting must begin from this blueprint and the accepted architecture state at that time. It may not silently redesign the platform, couple lighting to audio internals, start implementation before the audio completion gate, or treat illustrative names as permission to bypass later ADR review.

Architecture changes require an explicit proposal containing rationale, affected boundaries, migration impact, realtime impact, security impact, validation plan and rollback strategy.
