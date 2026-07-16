# Threading Model

## Audio Callback Thread

The CPAL output closure exclusively owns `RealTimeEngine` for the stream
lifetime. The callback may generate or map samples, update numeric object state,
render into caller-owned gains and scratch, apply in-place fractional delay,
interleave output, update fixed timing histogram storage, and publish numeric
metrics through relaxed atomics.

The callback must not allocate or resize containers, format strings, log, access
files or processes, enumerate devices, sleep, or acquire blocking locks. It does
not call `unwrap`, `expect`, `panic!`, or assertions. Buffer shapes are validated
with checked access. A failure fills the backend buffer with silence, increments
fixed counters, and publishes a `RealTimeFault` code.

When a host callback contains more frames than the configured processing block,
the same callback thread walks it in fixed-size borrowed chunks and a final tail.
No staging allocation or queue is introduced.

Renderer smoothing uses setup-sized numeric arrays. DSP history uses fixed
circular buffers. Test signal phase, noise filter history, and impulse state live
inside the engine and do not create per-block values with owned strings.

## Control Thread

The CLI thread loads scenes, allocates and configures the engine, enumerates and
opens devices, starts/stops the stream, prints status once per second, handles
speaker-identification confirmation, and coordinates shutdown. It reads only
atomic numeric snapshots while audio is running. Configuration that would resize
or rebuild engine state requires a controlled stream stop and replacement engine.

No general-purpose lock or unbounded queue connects these threads. Current
status delivery is bounded by fixed atomic fields; no runtime control message
requires callback delivery yet.

The Sprint 2A.1 duplex primitive adds one bounded contiguous SPSC frame ring. The input
callback exclusively owns `DuplexProducer`; the output callback exclusively owns
`DuplexConsumer`, compensator state, and fixed transition storage. Neither
callback transfers an owned block or waits for the other. The normal producer and
consumer paths publish one index per borrowed callback block. Live CPAL duplex
orchestration remains deferred at the review checkpoint.

Sprint 2B live orchestration assigns `DuplexProducer` to the CPAL input callback.
The output callback exclusively owns the adaptive consumer, Rubato adapter, PI
controller, fixed ASRC cache, input scratch, and `RealTimeEngine`. The control
thread alone prints status, writes soak/capture files, correlates latency, and
stops or reopens streams.

## Backend Ownership

CPAL streams remain on the thread that opened them because platform stream types
are not universally `Send`. Aurora public stream traits therefore do not require
`Send`; callback closures remain `Send`. CPAL and the host audio stack may use
internal synchronization outside Aurora's code, which Aurora cannot certify.

## Shutdown And Device Loss

Shutdown is cooperative: the control thread stops and drops the stream. The
callback never waits for the control thread. CPAL's backend error closure sets an
atomic device-loss flag. Engine block faults are separate numeric status codes;
the control thread owns user-facing formatting and controlled shutdown policy.

See `realtime-allocation-audit.md` for the pre-optimization call graph and the
violations removed during Optimization Sprint 1.
