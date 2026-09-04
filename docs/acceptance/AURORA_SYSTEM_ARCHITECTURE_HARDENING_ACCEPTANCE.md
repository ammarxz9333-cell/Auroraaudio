# Aurora System Architecture Hardening Acceptance

This matrix is mandatory before the corresponding capability is marked `production_ready`.

## A. Event Bus

- schema/version mismatch is rejected deterministically;
- invalid producer identity/permission is rejected and audited;
- 100k synthetic events do not leak memory or deadlock;
- bounded queue overflow follows documented policy;
- realtime audio callback remains nonblocking while the bus is saturated;
- service restart does not duplicate persistent state transitions incorrectly;
- event ordering rules are verified for source, playback, USB and plugin lifecycle events.

## B. Hardware Abstraction Layer

- Aurora Core builds/tests against a mock HAL without S6/STM32 headers;
- platform-specific failures return typed errors and safe states;
- mock loss of display, Wi-Fi, USB coprocessor and thermal sensors does not crash Core;
- hardware mute defaults to safe state on adapter initialization failure;
- channel map and clock-owner identity are explicit and validated;
- at least one second mock platform adapter demonstrates that Core APIs are not S6-specific.

## C. Thermal / Resource Manager

Software tests:
- deterministic pressure injection triggers the documented degradation order;
- library import and optional plugins throttle before realtime audio services;
- hysteresis prevents rapid oscillation between thermal states;
- invalid/missing sensor data enters conservative policy;
- resource limits cannot starve the realtime audio transport.

Physical S6 acceptance:
- representative Atmos/movie workload plus UI/network background tasks for at least 2 hours;
- record temperature, CPU frequency, critical-service CPU, xruns and USB resets;
- background work is throttled before any repeated audio deadline miss;
- no unsafe temperature policy override;
- if limits are exceeded, Aurora mutes/faults explicitly rather than emitting corrupted audio.

## D. Safe / Recovery Mode

- three consecutive injected normal-boot failures enter Safe Mode;
- failed update health check enters rollback/recovery path;
- crash-looping optional plugin is quarantined without blocking Safe Mode;
- Safe Mode starts with amplifiers muted and full audio engine stopped;
- diagnostics, rollback and scoped reset are accessible;
- scoped plugin/network/audio reset does not delete music library;
- known-good configuration can be restored;
- exit from Safe Mode requires a successful health gate or explicit reviewed user action.

## E. Built-in Test / Benchmark

- each configured 7.1.4 channel can be individually addressed in simulation;
- generated test signals have deterministic level/frequency and no accidental all-channel blast;
- USB transport diagnostics report CONFIG state, clocks, sequence errors and xruns;
- rear-node diagnostics report loss, jitter, drift and reconnect state when implemented;
- benchmark report contains commit SHA, config hash, versions, CPU, memory, thermals and xrun counts;
- report distinguishes simulated/host evidence from physical-hardware evidence;
- test interruption returns the system to muted/known source state safely;
- test mode cannot silently modify calibration or user media metadata.

## F. Integrated appliance gate

Before final `FLASH_READY` / `production_ready` release:
- all relevant automated tests pass in CI for the exact commit;
- S6 physical boot/display/touch/Wi-Fi/USB gates pass;
- STM32 and rear-node physical gates pass where those capabilities are enabled;
- safe update/rollback and Safe Mode recovery are exercised on the physical target;
- one full diagnostic and benchmark report is archived as release evidence;
- Capability Registry matches the shipped state and UI claims.
