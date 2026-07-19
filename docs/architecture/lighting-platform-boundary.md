# Aurora Lighting Platform Boundary

Status: Planned architecture boundary. Documentation only.

## Purpose

Aurora may later provide centrally managed whole-home lighting, cinema-reactive lighting, and first-party Aurora lighting hardware. This capability must not modify, depend on, or weaken the Aurora audio core.

The audio system and the lighting system are separate subsystems that may be deployed on the same physical computer for cost and simplicity, while retaining independent processes, contracts, lifecycle, failure handling, and future evolution.

## Non-negotiable boundary

Aurora Audio and Aurora Lighting are peer services under a shared control plane. Lighting is never part of the realtime audio callback, renderer, DSP pipeline, PCM contracts, transport clock, or physical audio-output path.

```text
                         Aurora App / CLI
                                |
                      Aurora Control Plane
                                |
                 +--------------+--------------+
                 |                             |
                 v                             v
        Aurora Audio Service          Aurora Lighting Service
                 |                             |
        Audio-owned contracts          Lighting-owned contracts
                 |                             |
        Renderer / DSP / CPAL          Rooms / scenes / devices
                 |                             |
              Speakers                   Lighting drivers
```

A failure, restart, overload, or disconnection in the lighting subsystem must not stop or degrade audio playback.

## Deployment model

Both services must support independent deployment.

### Same-device deployment

The initial recommended deployment may run both services on one Raspberry Pi or another supported host:

```text
Raspberry Pi
├── Aurora Control Plane
├── Aurora Audio Service
└── Aurora Lighting Service
```

Same-device deployment does not permit in-process coupling. The services remain isolated by process or equivalent service boundaries, communicate through bounded non-realtime IPC, and have separate supervision and restart policies.

### Separate-device deployment

A later installation may move lighting to another Raspberry Pi or dedicated Aurora lighting controller without changing audio-core code:

```text
Audio host                          Lighting host
├── Aurora Audio Service   <---->   ├── Aurora Lighting Service
└── Control endpoint               └── LED / lamp / relay drivers
```

Deployment location is configuration, not an architectural distinction.

## Ownership model

### Aurora Audio owns

- audio source ingestion;
- PCM and channel-layout contracts;
- realtime scheduling and callback safety;
- renderer and DSP pipelines;
- audio-device output;
- audio telemetry produced through a bounded, non-blocking handoff.

### Aurora Lighting owns

- rooms and lighting zones;
- device registry and capabilities;
- scenes, groups, brightness, colour and white-temperature state;
- lighting schedules and automations;
- cinema and music-reactive policies;
- lighting-device discovery, pairing and health;
- vendor and first-party hardware drivers;
- persistence of lighting configuration.

### Shared control plane owns

- authentication and authorization;
- application-facing command routing;
- service discovery;
- configuration references;
- health aggregation;
- API and protocol version negotiation.

The shared control plane must not contain audio DSP logic or lighting-effect logic.

## Lighting core evolution

Aurora must not prematurely place lighting domain logic inside the existing audio core. When lighting functionality becomes an implementation priority, it should receive its own core and contracts, for example:

```text
crates/
├── aurora-audio-core
├── aurora-audio-runtime
├── aurora-control-plane
├── aurora-event-contracts
├── aurora-lighting-core
├── aurora-lighting-runtime
└── aurora-lighting-drivers
```

Names are illustrative; the invariant is separation of ownership and dependency direction.

Allowed dependency direction:

```text
Audio service ------>
                     Shared versioned event/control contracts
Lighting service --->
```

Forbidden dependency direction:

```text
Aurora audio core -> Aurora lighting core
Aurora lighting core -> Aurora renderer or DSP internals
Lighting driver -> audio callback
Audio driver -> lighting-device implementation
```

## Event and synchronization boundary

Lighting may react to audio through published metadata or derived telemetry. It must never read or mutate realtime audio state directly.

```text
Audio callback
    |
    v
Bounded lock-free telemetry handoff
    |
    v
Audio telemetry worker
    |
    v
Versioned Aurora event contract
    |
    v
Lighting effect engine
    |
    v
Lighting driver
```

Candidate events include:

- playback state;
- playback timeline position;
- scene or preset selection;
- bounded channel-energy summaries;
- spectral-band summaries;
- beat or transient markers;
- movie-mode activation.

Events must contain explicit timestamps, sequence numbers and schema versions where synchronization matters. Dropped lighting events must never block the producer or affect audio continuity.

Raw PCM streaming to the lighting service is not part of the initial contract. Any future high-resolution analysis path requires a separate architecture decision, bounded resource budgets, privacy review and proof that it cannot affect realtime audio.

## Device abstraction

Lighting is modeled through capability-based devices rather than hard-coded RGB strips.

Examples of capabilities:

- on/off;
- dimming;
- RGB;
- RGBW or RGBIC segments;
- tunable white;
- zones or pixels;
- transitions;
- local effects;
- power and temperature telemetry.

The lighting core talks to Aurora-owned driver interfaces. Hardware-specific assumptions remain inside drivers.

```text
Aurora Lighting Core
        |
        v
Aurora Lighting Driver API
        |
        +-- Generic addressable LED driver
        +-- Network lighting driver
        +-- Future first-party Aurora controller driver
        +-- Optional third-party interoperability adapter
```

The initial architecture must not bind the platform to Govee, WS2812, a particular Raspberry Pi GPIO implementation, or any other vendor.

## Central home-lighting model

The application may expose a centralized hierarchy such as:

```text
Home
├── Cinema
│   ├── TV backlight
│   ├── Light bars
│   └── Ceiling zones
├── Living room
├── Bedroom
└── Garden
```

Core lighting entities should include:

- home or site;
- room;
- zone;
- device;
- group;
- scene;
- automation;
- capability;
- state and desired state.

This model supports whole-home management while keeping cinema-reactive behavior as one lighting feature, not the definition of the subsystem.

## Failure and resource isolation

Required behavior:

- lighting process crash: audio continues;
- lighting driver hang: driver is timed out or restarted without blocking audio;
- unavailable lighting node: commands fail explicitly and health becomes degraded;
- event backlog: old reactive events are dropped according to bounded policy;
- CPU pressure: lighting quality may degrade before audio deadlines are threatened;
- network loss: local audio continues and lighting reconnects independently;
- lighting configuration corruption: audio configuration remains readable and usable.

On a shared Raspberry Pi, the runtime should support resource budgets and process priority so that audio retains scheduling priority.

## Extension policy

Third-party lighting support must be implemented through drivers or external adapters. A third-party integration may not introduce its vendor model into the lighting core.

First-party Aurora lighting hardware should implement the same public capability and driver contracts used by compatible third-party devices. Product-specific enhancements may use versioned optional capabilities rather than private core hooks.

## Scope and sequencing

This document adds an architectural seam, not a current product commitment.

Current audio priorities remain unchanged:

1. complete the existing evaluation and cleanup work;
2. establish the local integrated-runtime vertical slice;
3. validate audio lifecycle and physical output;
4. add the previously approved audio foundations in isolated stages.

Lighting implementation begins only through a separately approved roadmap item and isolated PR series. No lighting dependency, crate, daemon, protocol, UI, GPIO access, device driver or realtime analyzer is added by this document.

## Acceptance criteria for future implementation

A lighting implementation is architecturally acceptable only when it demonstrates that:

- audio builds and runs with lighting entirely disabled or absent;
- lighting can run on the same host without entering the realtime path;
- lighting can move to a separate host through configuration;
- the lighting service can restart while audio continues;
- all cross-service messages use versioned Aurora-owned contracts;
- queues are bounded and backpressure cannot reach the audio callback;
- device-specific code remains behind lighting-driver interfaces;
- the audio core does not depend on lighting crates;
- simulation and physical validation results are reported separately.

## Explicit non-goals

This decision does not:

- convert AuroraAudio into a monolithic home-automation core;
- add lighting code to the audio engine;
- select LEDs, controllers or electrical hardware;
- promise Govee protocol compatibility;
- define a final consumer-product enclosure;
- implement whole-home automation;
- authorize copying proprietary or incompatible implementations.
