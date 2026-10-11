# Aurora Modular Speaker Fabric

Status: software control-plane plus fail-closed endpoint-contract baseline. Docked/wireless physical hardware is not yet validated.

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

## Atomic dock-to-wireless handover

A route change is not applied by simply flipping a module from docked to wireless. Aurora now builds an explicit handover epoch for every changed endpoint:

```text
MUTE all changed modules
        ↓
PREPARE new route / roles while silent
        ↓
ARM every changed endpoint for the same future media-frame
        ↓
COMMIT the new assignments
        ↓
audio may resume at the armed frame
```

The endpoint-side state machine rejects stale epochs and out-of-order Prepare/Arm/Commit commands. A removed module receives only Mute and remains fail-closed. This prevents one pod from continuing an old front role while another pod has already switched to a rear role.

The software probe exercises the complete two-pod transition from docked FL+TFL / FR+TFR to detached SL+TRL / SR+TRR and arms both endpoints to the same Aurora frame.


## Detachable endpoint firmware contract

Aurora now defines the MCU-facing safety state machine in
`aurora_modular_speaker_fabric::endpoint`. It is deliberately independent of
ESP-IDF, Wi-Fi and a particular amplifier so the same contract can be carried by
ESP32-P4/C6 or another endpoint processor.

Every module boots with the amplifier muted and follows:

```text
BOOT MUTED
   ↓
MUTE(new epoch)
   ↓
PREPARE(route + roles)
   ↓
ARM(shared future Aurora media frame)
   ↓
COMMIT (still muted)
   ↓
scheduled frame reached
   ↓
PLAY + unmute
```

The endpoint independently rejects stale epochs, wrong module IDs, unsupported
routes, too many output lanes, insufficient detached power, missing scheduled
playout, wrong clock source and excessive reported skew. Clock loss, transport loss, an attachment change during a prepared/active route,
or stream underflow immediately clears the active route and returns hardware
actions that mute the amplifier and stop playout. Initial dock detection while
boot-muted or already muted is accepted without latching a fault.

A fault is latched. Recovery only returns to a muted state; Aurora must issue a
fresh complete handover epoch before sound can resume. This prevents a stale
front/rear assignment from becoming audible after a reconnect.

The endpoint exposes a `SpeakerModuleState` snapshot using the same identity,
capabilities, attachment and synchronization evidence consumed by Aurora OS, so
the device-side and core-side admission models share one truth boundary.

Run the deterministic endpoint probe:

```bash
cargo run -p aurora-modular-speaker-fabric --example endpoint_fail_closed
```

This probe is software evidence only. It does not claim that a particular
ESP32 radio, PTP implementation, DAC, Class-D stage, battery pack or dock has
met the physical timing/electrical acceptance gates.



## Endpoint scheduler telemetry

The exact-pinned ESP-AVB endpoint build can expose Aurora handover scheduler
telemetry through the existing serialized `avb_status()` path. The telemetry
includes the active handover epoch, rendered roles, requested absolute PTP
activation time, the PTP timestamp at which firmware requested output unmute,
scheduler unmute count, deadline-fault count, endpoint phase, and logical mute
state.

The companion snapshot helper emits:

`aurora.esp-avb.endpoint-control-snapshot.v1`

This telemetry is intentionally separated from physical acceptance. The field
`scheduler_unmute_ptp_ns` records firmware/control-plane timing only. It is
**not** the measured DAC/Class-D/speaker electrical activation timestamp and
cannot satisfy the physical acceptance harness by itself. A real M2/M3 trial
must correlate it with an independent electrical capture.


## Physical acceptance harness

Physical endpoint trials are evaluated by:

```bash
python validation/physical/aurora_detachable_physical_acceptance.py module \
  --input left-pod.json \
  --max-start-error-us 1000

python validation/physical/aurora_detachable_physical_acceptance.py pair \
  --left left-pod.json \
  --right right-pod.json \
  --max-start-error-us 1000 \
  --max-pair-skew-us 1000 \
  --output pair-report.json
```

A module trial cannot pass from advertised capabilities alone. The evidence must
contain a measured electrical activation timestamp, one continuously locked gPTP
epoch, progressing receive counters, zero pre-roll, zero underflows, and observed
hardware mute behavior before activation, on STOP/MUTE, and on sync loss.

A two-module `PHYSICAL-PASS` additionally requires distinct module and stream
identities, one shared handover epoch, one shared PTP grandmaster, one requested
absolute PTP activation time, and measured inter-module electrical skew inside
the explicitly selected threshold.

The default 1000 microsecond limits mirror the V1 cinema admission policy only;
they are not a claim that this is the final perceptual target. Tighter limits can
be selected for later hardware qualification. The built-in `self-test` uses
synthetic fixtures and therefore prints `physical-pass=synthetic-only`; it is
only a schema/fail-closed CI test.


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
