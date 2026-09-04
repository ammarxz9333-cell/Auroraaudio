# Aurora reference-project parity acceptance

Status: acceptance contract. **Architecture or host-test superiority is not physical-product superiority.**

Aurora reuses proven specialist components instead of reimplementing their core jobs. This document defines what must be demonstrated before saying the integrated appliance behaves at least as well as the reference projects from which its design is derived.

## Evidence classes

- **INHERITED** — Aurora executes the pinned upstream component for that function; Aurora must prove it does not corrupt or bypass the upstream contract.
- **HOST-PASS** — deterministic CI proves the Aurora integration behavior on a host machine.
- **PHYSICAL-PASS** — the exact S6 + selected realtime MCU + eARC/output hardware has measured evidence.
- **PENDING** — required behavior is not yet demonstrated at the necessary evidence class.

No HOST-PASS item may be promoted to PHYSICAL-PASS by documentation, simulation, or a local-file test.

## Reference matrix

| Reference | Behavior Aurora must preserve or exceed | Aurora owner | Required evidence | Current status |
|---|---|---|---|---|
| Omniphony | persistent IEC61937 parsing, object/VBAP render, canonical 7.1.4 ordering | pinned Omniphony v0.5.2 + Harletty bridge | real JOC fixture + live-service physical run | **INHERITED / HOST-PASS; PHYSICAL-PENDING** |
| Omniphony realtime output | bounded output latency, no long-term FIFO growth/xrun under a physical clock | Aurora 40-frame pipe + postprocessor + USB clock reports | measured queue/latency/xrun counters on S6 hardware | **HOST-PASS; PHYSICAL-PENDING** |
| Harletty | E-AC-3 JOC/OAMD decode correctness and decoder recovery | pinned Harletty v0.7.3 | upstream decoder tests plus Aurora real-JOC fixture; commercial-service soak later | **INHERITED; PHYSICAL-PENDING** |
| Vibesbox | authorized TV/eARC DD+ reaches the computer intact through Lindy 38368 / SiI9437 tap | realtime-MCU serial-audio capture | logic-analyzer bytes + IEC61937 preamble/data type + long capture | **portable HOST-PASS; PHYSICAL-PENDING** |
| Vibesbox source routing | sources cannot corrupt each other during transitions | Aurora source manager + exclusive final mux | CONFIG ownership, REVOKE/QUIESCED, watchdog, repeated switch test | **HOST-PASS** |
| CamillaDSP | independent capture/playback clocks do not cause unbounded queue drift | `CLOCK_REPORT` → shared DriftController → Rubato ASRC | deterministic drift tests + physical long soak | **HOST-PASS; PHYSICAL-PENDING** |
| CamillaDSP | output remains bounded under gain/crossover/DSP changes | Aurora postprocessor headroom + linked limiter | finite/peak-bound unit tests + physical speaker tuning | **HOST-PASS; PHYSICAL-PENDING** |
| Snapcast | timestamp/clock model keeps remote speakers coherently synchronized | Aurora remote-clock estimator / future rear transport | network impairment test + measured inter-speaker skew | **estimator HOST-PASS; transport PENDING** |

## Non-negotiable ownership rules

### 1. One IEC61937 parser

Omniphony remains the only persistent IEC61937 burst parser in the S6 immersive path. The realtime MCU only normalizes the physical carrier into canonical S16_LE words; `aurora-live-ingest` forwards those bytes unchanged. Adding another Dolby/IEC burst parser in Aurora would be a regression unless the architecture is explicitly changed and revalidated.

### 2. One final PCM owner

Only the source granted by `aurora-source-manager` may configure and feed the realtime-MCU PCM timeline. `aurora-source-gate` owns the single final mux and CONFIG-ACK ownership. Source data sockets carry Aurora USB `AUR0` frames only; source-manager `ASC0` control messages must use a separate control path and must never be multiplexed into a decoder data socket.

This separation is a hard acceptance gate because an `ASC0` packet on the HDMI/live-ingest data socket is not an Aurora USB frame and would otherwise force decoder recovery.

### 3. One clock-correction owner

The physical playback clock belongs to the realtime MCU. S6 receives `CLOCK_REPORT`; one Aurora `DriftController` drives one shared 12-channel Rubato ASRC ratio. No second independently acting sample-rate servo may be inserted into the same live path.

### 4. One post-render protection path

Bass management, headroom, limiter and lip-sync belong in the Aurora postprocessor. The final source gate may ramp/mute for source ownership but must not duplicate crossover, limiter, ASRC or lip-sync delay.

### 5. Fail closed on ownership or hardware-session loss

Manager loss, USB session loss, CONFIG rejection, malformed protocol, playback XRUN, eARC clock loss or VBUS fault must leave output silent until a fresh valid ownership/configuration path is established.

## Host parity gates

The S6 CI must remain green for all of these before a parity claim is allowed:

1. shared Aurora USB stream parser fragmentation/reassembly;
2. source priority and REVOKE/QUIESCED arbitration;
3. stalled-source quiesce watchdog;
4. exclusive Local/HDMI final mux and cached CONFIG ownership;
5. no PCM before valid realtime-MCU CONFIG ACK;
6. manager-loss and USB-reset fail-closed behavior;
7. source-control acknowledgement semantics;
8. live IEC61937 broker startup/discontinuity/reconnect recovery;
9. real postprocessor integration with ASRC and 40-frame periods;
10. postprocessor finite-output/linked-limiter/lip-delay/clock-correction tests;
11. realtime-MCU transport, eARC normalization and HAL recovery tests;
12. wiring audit proving no parallel renderer/DSP/ASRC path;
13. strict compile warnings and Rust lint gates for the new realtime surfaces.

A red current CI invalidates the HOST-PASS parity claim even if an older run was green.

## Physical parity gates

Aurora is **not** equivalent to a field-proven appliance until the exact target hardware passes:

1. S6 appliance boot/display/touch/Wi-Fi validation;
2. USB HS host ↔ S6 gadget repeated cold-boot and reconnect cycles;
3. Lindy/SiI9437 → realtime-MCU capture measured at 48/96/192 kHz as applicable;
4. commercial DD+ JOC service input with JOC/OAMD/object evidence and height-channel activity;
5. 12-channel physical TDM/DAC output with canonical channel map;
6. measured A/V latency and stable user lip-sync correction;
7. long clock-drift/xrun/thermal soak with bounded queue level;
8. stereo ↔ DD+ 5.1 ↔ DD+ JOC transitions without pop, stale PCM or reboot;
9. repeated source switching while audio is active;
10. rear-node network synchronization under packet loss/jitter/interference before any Snapcast-class claim.

The detailed commercial-streaming gate remains `docs/AURORA_LIVE_STREAMING_ATMOS_ACCEPTANCE.md`.

## Comparative claim policy

Aurora may say **architecturally stronger** where it adds deterministic CI, explicit ownership, fail-closed state machines or a stricter hardware manifest than a reference project. It may say **host-validated parity** only when the matching CI gate is green. It may say **physically equal or better** only after measured hardware evidence satisfies the physical gates above.

Until then, the correct statement is: Aurora deliberately combines the specialist strengths of the reference projects and adds stricter integration/recovery contracts, but final appliance parity remains dependent on physical S6/realtime-MCU/eARC/output and wireless-rear validation.
