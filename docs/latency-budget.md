# Latency Budget

Aurora has two distinct software deadlines in the current direct-eARC path and
they must not be mixed into one misleading number.

- A canonical E-AC-3 IEC61937 repetition period is 24,576 bytes on the historical
  192 kHz / two-slot / S16 carrier: **32 ms per complete access unit**.
- The canonical speaker DSP block is 40 frames at 48 kHz: **0.8333 ms per block**.

The existing realtime policy remains average processing below 25% and maximum
processing below 75% of the applicable scheduling interval. For a 40-frame DSP
block that means 208 us average and 625 us maximum. For work performed once per
32 ms E-AC-3 access unit, the corresponding *shared* AU processing envelope is
8 ms average and 24 ms maximum. The latter is a combined envelope, not an 8/24 ms
allowance independently granted to parser, decode and JOC render.

## Per-stage validation table

`aurora-sim-source::latency::StageLatencyBook` provides fixed-capacity,
allocation-free recording with p50, p99, exact max and an explicit overflow
counter. Its 100 us buckets cover values through 51.2 ms; values above that range
remain visible through `overflow_samples` and exact `max_us` instead of silently
clamping away the miss.

| Stage | Measurement boundary | p50 | p99 | Max | Budget / target | State |
| --- | --- | ---: | ---: | ---: | --- | --- |
| capture | source read/copy boundary | — | — | — | Must not consume the 40-frame callback deadline; physical blocking/device latency is reported separately | NOT MEASURED on current head |
| IEC61937 parser | immediately around transport parsing | — | — | — | Shares the 8 ms average / 24 ms max 32-ms AU envelope with decode/render | collector implemented; wiring pending |
| decode | complete-AU decoder call and drain | — | — | — | Shares the 8 ms average / 24 ms max AU envelope | collector implemented; wiring pending |
| JOC render | OpenJOC render substage | runtime exposes latest/max | runtime exposes no percentile yet | runtime exposes max total, not render max | Shares AU envelope; `0x15` alone is never a JOC timing sample | live timing exists; histogram aggregation pending |
| SpeakerPostProcessor | canonical 40-frame block | **11.1 us** | **13.3 us** | **118 us** | <208 us average, <625 us max for the 40-frame interval | PROVEN prior host checkpoint; not exact-head rerun |
| output | sink write boundary | — | — | — | Blocking/device latency must be separated from CPU processing; XRUNs are a separate failure metric | NOT MEASURED on current head |
| total software chain | source boundary through accepted sink write | — | — | — | Must sustain every 32 ms AU without backlog and every 0.833 ms speaker block without deadline miss | NOT MEASURED |

The 11.1/13.3/118 us SpeakerPostProcessor result is the prior executed host
self-test recorded by the pre-hardware checkpoint. It excludes decoder, carrier,
ASRC, operating-system scheduling and hardware. It must not be presented as
end-to-end latency.

## Histogram semantics

The hot recording operation stores only integer counters in preallocated arrays.
It performs no formatting, file I/O, locking or sample-vector growth. A dedicated
allocation regression measures repeated parser/decode/DSP stage recordings and
requires zero heap allocations. Reporting and text/JSON formatting are explicitly
outside the realtime measurement boundary.

A percentile is returned as the upper edge of the matching 100 us bucket. This is
an intentional bounded-resolution metric rather than falsely precise nanosecond
telemetry. `max_us` remains the exact observed integer-microsecond maximum.

## Algorithmic versus wall-clock latency

Fixed algorithmic latency includes renderer latency and configured DSP delay.
Control-rate block midpoint updates may add up to half a block of position
quantization without buffering PCM. ALSA/WASAPI device periods, kernel queues,
DAC latency, speaker propagation and physical loopback are different quantities
and must not be inferred from software CPU timings.

For the headless harness, file transport can measure parser/decode/render/DSP CPU
work but has no meaningful ALSA XRUN metric. An XRUN assertion is valid only when
an ALSA loopback or physical device is actually participating. Only accepted
captured-loopback correlation may be called measured round-trip latency.

## Required report gate

The eventual `aurora-sim latency-report` output must include, for every stage:
`sample_count`, `p50_us`, `p99_us`, `max_us`, `overflow_samples`, the applicable
budget domain, and whether the stage was actually observed. Missing stages are
reported as **NOT MEASURED**, never as zero latency.
