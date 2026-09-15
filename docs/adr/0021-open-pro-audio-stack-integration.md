# ADR 0021: Open pro-audio stack integration boundaries

Status: proposed and implementation-started on `open-audio-stack-integration`

## Context

Aurora already owns decoder, scene, renderer, DSP, audio-I/O, runtime and realtime boundaries. New upstream candidates must strengthen those layers without becoming a second architecture or introducing competing clocks, resamplers, DSP graphs, or realtime-unsafe I/O.

The evaluated candidates in `config/open-audio-stack-v1.json` are:

- AOO for peer/UDP network audio;
- NXP GenAVB/TSN for wired AVB/TSN/Milan on supported NXP platforms;
- Sound Open Firmware (SOF) for i.MX8M Plus HiFi4 system DSP;
- VideoLAN libspatialaudio as a possible production spatial renderer.

RoomEQ, EBU EAR/libear, SAF, OAR and CamillaDSP already have repository-defined reference or external-executor boundaries. Those boundaries are intentionally retained.

## Decision

### 1. Aurora owns the media timeline

Aurora exposes absolute media timestamps as frame indexes in an explicit sample-rate domain. The canonical immersive network rate is 48 kHz until a later ADR changes it.

A backend can map that timeline to gPTP/PTP or track a peer clock, but it cannot silently redefine Aurora time.

### 2. Network I/O never runs in the audio callback

`aurora-realtime-audio-api::NetworkAudioTransport` is a worker-thread contract. The callback communicates with a transport worker only through bounded preallocated queues. Socket I/O, retransmission, discovery, logging and configuration remain off the callback.

### 3. One rate controller per clock-domain crossing

For an AVB/PTP endpoint, the PTP/media-clock mapping owns the crossing. For an asynchronous peer/wireless endpoint, one adaptive rate matcher owns the crossing. PipeWire, AOO, Aurora ASRC and hardware DSP must not all correct the same drift in series.

A prepared path that would activate more than one adaptive rate controller for the same crossing must fail closed.

### 4. Wired and wireless fan out after system DSP

The common production pipeline remains conceptually:

`ingress -> decode -> scene -> renderer -> system DSP -> Aurora PCM bus`

Only then does it fan out to local outputs, wired network transport, or wireless network transport. This prevents endpoint transports from changing authored/object semantics or room-correction intent.

### 5. AOO is a wireless/peer transport candidate, not Aurora's clock owner

AOO is integrated only behind the Aurora network transport worker boundary. Its useful mechanisms include timestamped PCM, jitter handling, retransmission and adaptive peer-clock compensation. Aurora retains the right to disable or replace AOO resampling when another clock-domain controller owns the crossing.

The exact upstream pin is recorded in `config/open-audio-stack-v1.json`. A runtime AOO adapter cannot be marked selected until it has deterministic packet-loss, drift, reconnect and sustained-soak evidence through Aurora's own contracts.

### 6. GenAVB/TSN is the preferred wired pro-audio candidate on supported NXP hardware

GenAVB/TSN is platform-gated. Generic CI may validate source provenance, configuration semantics and Aurora adapter behavior, but it cannot claim i.MX8MP hardware timestamping, gPTP lock, Milan interoperability or physical latency.

On supported hardware, GenAVB/TSN may own the wired PTP/network-media mapping while Aurora remains the owner of the logical media timeline and channel semantics.

### 7. SOF is a DSP execution target, not a second Aurora DSP policy engine

RoomEQ or another Aurora-approved optimizer may generate correction intent. Aurora owns the semantic DSP graph and coefficients. SOF may execute supported FIR/IIR/DRC/SRC operations on the i.MX8MP HiFi4 DSP after a translation/validation step.

Unsupported graph semantics must fail closed or use an explicitly selected software fallback; they must not be silently approximated.

### 8. libspatialaudio is a renderer candidate, not automatically the production renderer

Aurora currently has native renderer implementations and independent reference lanes. libspatialaudio may be selected only after an adapter satisfies Aurora's allocation/caller-owned-buffer contract and passes deterministic semantic differentials against the existing Aurora renderer plus independent EAR/SAF/OAR evidence where applicable.

Because libspatialaudio is LGPL-2.1-or-later, integration should preserve a replaceable library boundary rather than copying its implementation into MIT-licensed Aurora core.

Only one production speaker renderer processes a block. Reference renderers remain out of the steady-state chain.

## Initial implementation

This ADR introduces the first concrete shared boundary in `aurora-realtime-audio-api`:

- `MediaTimestamp`;
- `NetworkAudioFormat` and exact shape validation;
- `NetworkTimingPolicy`;
- explicit clock-discipline modes;
- transport capabilities;
- `NetworkAudioBlock`;
- `NetworkAudioTransport`, documented as worker-thread-only.

These types intentionally contain no AOO, GenAVB, PTP, RTP or SOF types.

## Validation gates before runtime selection

A network backend must demonstrate:

1. exact 48 kHz / declared-channel block semantics;
2. monotonic timestamps and sequence behavior;
3. bounded overflow/underflow handling;
4. packet-loss/reorder/reconnect fault evidence when applicable;
5. sustained drift convergence without double resampling;
6. deterministic channel mapping through at least 7.1.4;
7. no allocation/blocking/network I/O from Aurora's audio callback;
8. truthful distinction between simulation, software runtime and physical evidence.

A renderer backend must additionally prove caller-owned steady-state storage, bounded latency and semantic agreement on the repository's declared object/layout corpus.

A SOF target must additionally prove coefficient/route round-trip, fail-closed unsupported operations, and physical i.MX8MP evidence before being described as hardware validated.

## Consequences

This keeps Aurora hardware-neutral while allowing high-quality platform acceleration. AOO, GenAVB/TSN, SOF and libspatialaudio remain replaceable. No external upstream can force Aurora to adopt its timing model, buffer ownership, object model or public API.
