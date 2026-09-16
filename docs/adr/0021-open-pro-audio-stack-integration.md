# ADR 0021: Open pro-audio stack integration boundaries

Status: implemented baseline; embedded ESP endpoint evaluation added on `esp-avb-ptp-integration`

## Context

Aurora already owns decoder, scene, renderer, DSP, audio-I/O, runtime and realtime boundaries. New upstream candidates must strengthen those layers without becoming a second architecture or introducing competing clocks, resamplers, DSP graphs, or realtime-unsafe I/O.

The evaluated candidates in `config/open-audio-stack-v1.json` are:

- AOO for peer/UDP network audio;
- NXP GenAVB/TSN for wired AVB/TSN/Milan on supported NXP platforms;
- Scramble Tools `esp_avb` + `esp_ptp` for embedded ESP32-P4/C6 AVB speaker-endpoint and PTP/gPTP experiments;
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

The exact upstream pin is recorded in `config/open-audio-stack-v1.json`. Runtime selection remains explicit and fail-closed.

### 6. GenAVB/TSN is the preferred wired pro-audio candidate on supported NXP hardware

GenAVB/TSN is platform-gated. Generic CI may validate source provenance, configuration semantics and Aurora adapter behavior, but it cannot claim i.MX8MP hardware timestamping, gPTP lock, Milan interoperability or physical latency.

On supported hardware, GenAVB/TSN may own the wired PTP/network-media mapping while Aurora remains the owner of the logical media timeline and channel semantics.

### 7. ESP-AVB and ESP-PTP are embedded speaker-endpoint candidates, not a 7.1.4 transport claim

Aurora pins Scramble Tools `esp_avb` and `esp_ptp` as an experimental embedded endpoint path for ESP32-P4/C6 hardware. `esp_avb` supplies AVB talker/listener and Ethernet/Wi-Fi endpoint/bridge mechanisms, while `esp_ptp` supplies IEEE 1588 PTP / IEEE 802.1AS gPTP time mapping for those endpoints.

They remain behind Aurora's `NetworkAudioTransport` worker boundary. Their PTP/gPTP clock may map Aurora media time to endpoint/network time, but it never becomes Aurora's logical media-clock owner.

At the pinned `esp_avb` revision the upstream-declared streaming profile is AAF PCM, 24-bit, 48 kHz, with one talker plus one listener and at most two channels per stream. Therefore this integration is a stereo speaker-node candidate only. Aurora must not describe it as a complete 7.1.4 path until multi-endpoint channel mapping, cross-endpoint synchronization, loss/reconnect behavior, RF resilience and physical latency have been demonstrated on real hardware.

Generic CI validates exact source pins and declared source contracts only. It does not build an ESP-IDF firmware image or prove P4 hardware timestamps, C6 software-clock discipline, Wi-Fi synchronization, bridge QoS, or acoustic speaker synchronization.

### 8. SOF is a DSP execution target, not a second Aurora DSP policy engine

RoomEQ or another Aurora-approved optimizer may generate correction intent. Aurora owns the semantic DSP graph and coefficients. SOF may execute supported FIR/IIR/DRC/SRC operations on the i.MX8M Plus HiFi4 DSP after a translation/validation step.

Unsupported graph semantics must fail closed or use an explicitly selected software fallback; they must not be silently approximated.

### 9. libspatialaudio is a renderer candidate, not automatically the production renderer

Aurora currently has native renderer implementations and independent reference lanes. libspatialaudio remains behind Aurora-owned object-PCM and control-plane boundaries.

Because libspatialaudio is LGPL-2.1-or-later, integration preserves a replaceable library boundary rather than copying its implementation into MIT-licensed Aurora core.

Only one production speaker renderer processes a block. Reference renderers remain out of the steady-state chain.

## Initial implementation

The shared network boundary in `aurora-realtime-audio-api` includes:

- `MediaTimestamp`;
- `NetworkAudioFormat` and exact shape validation;
- `NetworkTimingPolicy`;
- explicit clock-discipline modes;
- transport capabilities;
- `NetworkAudioBlock`;
- `NetworkAudioTransport`, documented as worker-thread-only.

These types intentionally contain no AOO, GenAVB, ESP-AVB, ESP-PTP, PTP, RTP or SOF types.

The open-audio-stack validation lane exact-pins `esp_avb` and `esp_ptp`, checks their declared component versions/licenses, verifies the pinned stereo AAF profile and target families, and fails closed if their source revisions drift.

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

An ESP-AVB/ESP-PTP endpoint path must additionally prove multi-endpoint synchronization for the intended speaker count, explicit channel-to-node mapping, real RF/loss/reconnect behavior, and physical timing evidence before selection for immersive speaker output.

A renderer backend must additionally prove caller-owned steady-state storage, bounded latency and semantic agreement on the repository's declared object/layout corpus.

A SOF target must additionally prove coefficient/route round-trip, fail-closed unsupported operations, and physical i.MX8MP evidence before being described as hardware validated.

## Consequences

This keeps Aurora hardware-neutral while allowing high-quality platform acceleration. AOO, GenAVB/TSN, ESP-AVB/ESP-PTP, SOF and libspatialaudio remain replaceable. No external upstream can force Aurora to adopt its timing model, buffer ownership, object model or public API.
