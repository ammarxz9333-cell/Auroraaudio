# Aurora Modular Speaker Fabric

Status: software control-plane baseline. Docked/wireless physical hardware is not yet validated.

## Product goal

Aurora is not limited to a fixed soundbar topology. A speaker module can be:

- mechanically docked into the soundbar and driven by a shared wired PCM/clock bus;
- detached and reassigned as a synchronized rear/surround speaker;
- paired with another module for stereo playback;
- used by itself as a portable/standalone speaker;
- admitted to a multiroom deployment when the selected transport meets that mode's timing policy.

The same physical module identity survives every transition.

## Core rule: detaching does not guess room position

Mechanical detachment alone is not enough to change a speaker role.

A stored preset, app/calibration action, or future positioning system explicitly requests the new logical role. This prevents a module that was merely removed for charging or portable use from silently becoming a rear speaker.

Example:

```text
Docked:
[left-pod]  [Aurora core bar]  [right-pod]
    FL                              FR
    |                               |
 shared dock PCM + shared dock clock

Detached cinema preset:
                listener
          SL                 SR
      left-pod           right-pod
          \                 /
          scheduled network playout
```

## V1 control-plane implementation

`aurora-modular-speaker-fabric` owns:

- stable module identity;
- docked vs detached state;
- static endpoint capabilities;
- requested speaker-role assignment;
- dock/wired-network/wireless-network route selection;
- clock source and synchronization evidence;
- fail-closed admission for cinema/stereo/multiroom modes;
- deterministic reconfiguration plans.

For detached assignments, the fabric also materializes Aurora network routes:
- source channel indexes are taken from the canonical 7.1.4 rendered PCM bus;
- PTP endpoints become `PtpFollower` network streams;
- adaptive-peer endpoints become `AdaptiveRateFollower` streams;
- docked assignments remain on the synchronous local dock path.

A stateful `FabricSession` reconciles hot dock/undock, disappearance, and sync changes. If a previously active module loses synchronization or disappears, the active plan is cleared immediately instead of retaining a stale route.

It deliberately does **not** own:

- Wi-Fi drivers;
- AVB/PTP implementation;
- amplifier/DAC drivers;
- battery management;
- charging;
- mechanical dock detection;
- acoustic localization/calibration;
- Dolby decoding.

Those remain behind separate Aurora adapters or future endpoint firmware.

## Dock contract

A production docked module should expose the following logical interfaces:

1. power/charging;
2. deterministic module identity;
3. dock-present state;
4. digital PCM path;
5. shared media clock or clock derived from the same dock bus;
6. control/telemetry path;
7. fault/mute path.

The exact connector and electrical layer are intentionally not frozen by this software crate. I2S/TDM, a framed serial bus, USB audio, or another synchronous digital bus can satisfy the contract if the physical implementation proves channel mapping and common-clock behavior.

Docked cinema admission requires:

- module online;
- dock PCM capability;
- explicit `DockRecovered` clock state;
- locked synchronization.

## Detached contract

A detached module needs its own local endpoint chain:

```text
scheduled network PCM
        |
 clock/timestamp discipline
        |
 bounded receiver/jitter buffer
        |
 local DSP / crossover / limiter
        |
 DAC / Class-D
        |
 speaker drivers
        |
 battery / power management
```

Cinema mode does not accept "audio arrives over Wi-Fi" as sufficient proof. The endpoint must report:

- clock lock;
- scheduled playout support;
- a known media-clock relationship;
- bounded estimated skew inside the configured policy.

The V1 default software policy admits up to 1000 microseconds of reported skew for cinema/stereo and 10000 microseconds for multiroom. These are Aurora admission-policy defaults, not claims about any current ESP hardware.

## Existing Aurora pieces reused

Aurora already contains:

- a timestamped network-audio API;
- explicit clock-discipline modes;
- bounded network transport queues;
- AOO transport work;
- ESP-AVB endpoint planning;
- PTP/gPTP-oriented ESP32-P4/C6 evaluation;
- canonical 7.1.4 channel roles;
- native 3D rendering.

The modular fabric sits above these pieces. It decides **which physical module should reproduce which rendered role** and only admits a detached route when synchronization evidence is good enough.

## Initial proven software transition

The first deterministic example covers the same physical pair changing use:

```text
state A:
left-pod  = docked / DockBus / FL
right-pod = docked / DockBus / FR

state B:
left-pod  = detached / WirelessNetwork / SL
right-pod = detached / WirelessNetwork / SR
```

State B is accepted only when both endpoint clocks are locked and scheduled playout/skew evidence passes policy. A two-lane detachable Atmos pod can carry both a horizontal surround lane and an up-firing/top lane: the left pod becomes SL+TRL and the right pod becomes SR+TRR. In Aurora's canonical 7.1.4 bus those pairs materialize source indexes [4,10] and [5,11] (zero-based) into timestamped two-channel network streams. This lets the same physical side pods serve front/top-front roles while docked and surround/top-rear roles after they are moved behind the listener.

Run:

```bash
cargo run -p aurora-modular-speaker-fabric --example detachable_reconfiguration
```

## Physical milestones

### M1 — dock proof
One physical module:
- docks and is detected;
- receives digital audio and common clock;
- charges;
- undocks without corrupting the Aurora media timeline.

### M2 — one wireless module
One detached module:
- receives timestamped 48 kHz PCM;
- exposes clock-lock/skew telemetry;
- survives loss/reconnect;
- fails closed rather than playing late/unsynchronized audio.

### M3 — synchronized detachable pair
Two modules:
- become SL/SR;
- scheduled playout remains aligned for a sustained run;
- measured electrical/acoustic skew stays inside the selected acceptance threshold.

### M4 — mixed dock + detached cinema
Central/front modules remain docked while detached modules operate as real rears. Renderer output, role assignment, endpoint timestamps, and physical channel identity must all agree.

### M5 — full modular immersive system
Multiple detachable modules may be assigned to rear/height/standalone/multiroom roles without changing Aurora decoder/renderer semantics.

## Truth boundary

Current implementation proves topology logic and fail-closed synchronization admission in software only.

It does not yet prove:

- ESP32 wireless sample-accurate cinema synchronization;
- a detachable battery/amplifier PCB;
- dock charging or hot-plug behavior;
- physical rear-speaker latency;
- automatic acoustic positioning;
- Dolby certification;
- a complete production soundbar enclosure.
