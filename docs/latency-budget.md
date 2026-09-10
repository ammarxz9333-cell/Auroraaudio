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

## Headless measurement command

`aurora-sim latency-report` now supplies the software-only report path. With no
input file it asks local FFmpeg to generate ordinary 48 kHz / 5.1 E-AC-3, frames
that stream into complete AUs through the pinned OpenJOC framing contract, wraps
each AU into the canonical IEC61937 repetition period, and measures the software
stages that can be isolated without audio hardware.

Example:

```text
cargo run -p aurora-cli --no-default-features --features validation --bin aurora-sim -- \
  latency-report --seconds 0.256 --iterations 4
```

The command deliberately reports real ALSA capture and output-sink stages as
`NOT_MEASURED`. File I/O or a zero-filled stand-in is not relabeled as hardware
latency. A positive JOC render timing sample is recorded only when OpenJOC's
complete-AU classifier has actually classified the stream as JOC; IEC61937 type
`0x15` is never sufficient.

## Per-stage validation table

`aurora-sim-source::latency::StageLatencyBook` provides fixed-capacity,
allocation-free recording with p50, p99, exact max and an explicit overflow
counter. Its 100 us buckets cover values through 51.2 ms; values above that range
remain visible through `overflow_samples` and exact `max_us` instead of silently
clamping away the miss.

| Stage | Measurement boundary | Current recorded p50 | Current recorded p99 | Current recorded max | Budget / target | State |
| --- | --- | ---: | ---: | ---: | --- | --- |
| capture | real source/device capture read | — | — | — | Physical blocking/device latency reported separately; queue starvation/XRUN is a failure metric | **NOT MEASURED** by file harness |
| IEC61937 parser | immediately around one canonical `BurstParser::push()` period | — | — | — | Shares 8 ms average / 24 ms max 32-ms AU envelope with decode/render | report wiring implemented; exact-head result pending |
| decode | complete-AU decoder admission plus empty-input drain polls | — | — | — | Shares 8 ms average / 24 ms max AU envelope | report wiring implemented; exact-head result pending |
| JOC render | OpenJOC render timing exposed by positive classified JOC AU | — | — | — | Shares AU envelope; non-JOC E-AC-3 contributes no JOC-render sample | report aggregation implemented; positive fixture execution pending |
| SpeakerPostProcessor | canonical speaker-domain output DSP call | **11.1 us** | **13.3 us** | **118 us** | <208 us average, <625 us max for 40-frame interval | PROVEN prior host checkpoint; exact-head rerun pending |
| output | real sink write / device submission | — | — | — | Device/blocking latency separated from CPU; XRUNs reported independently | **NOT MEASURED** by file harness |
| software-chain sample | parser start through decoded-frame drain/DSP for one supplied AU | — | — | — | Must sustain every 32 ms input period without backlog | implemented in `aurora-sim latency-report`; exact-head result pending |
| physical end-to-end | capture edge through real output observation | — | — | — | Product acceptance requires measured rig result | **NOT PROVEN** |

The 11.1/13.3/118 us SpeakerPostProcessor result is the prior executed host
self-test recorded by the pre-hardware checkpoint. It excludes decoder, carrier,
operating-system scheduling and hardware. It must not be presented as end-to-end
latency.

No parser/decode/JOC/software-chain numbers are written into this document until
the command has actually executed on a named head SHA. Source-defined collectors
are not substituted for measurements.

## Histogram semantics

The hot recording operation stores only integer counters in preallocated arrays.
It performs no formatting, file I/O, locking or sample-vector growth. A dedicated
allocation regression repeatedly records parser/decode/DSP stage values and
requires zero heap allocations. Reporting and text formatting are explicitly
outside the realtime measurement boundary.

A percentile is returned as the upper edge of the matching 100 us bucket. This is
an intentional bounded-resolution metric rather than falsely precise nanosecond
telemetry. `max_us` remains the exact observed integer-microsecond maximum and
`overflow_samples` keeps deadline-scale excursions visible.

## Stress timing relationship

`aurora-sim stress` paces the headless E-AC-3 soak at the nominal 32 ms period by
default. `--unpaced` exists only for CI smoke execution; results from an unpaced
run are correctness/stability checks and must not be quoted as realtime cadence
proof. Full acceptance keeps the default pacing for 1800 seconds and applies the
5% post-warmup RSS-growth ceiling.

The stress harness allocates its carrier, S32 slot, idle and jitter buffers before
the soak loop. Period construction and carrier-to-S32 conversion reuse those
buffers. `/proc/self/status` RSS reads occur as an out-of-band observer every 128
periods and are not part of the realtime audio measurement path.

## Algorithmic versus wall-clock latency

Fixed algorithmic latency includes renderer latency and configured DSP delay.
Control-rate block midpoint updates may add up to half a block of position
quantization without buffering PCM. ALSA/WASAPI device periods, kernel queues,
DAC latency, speaker propagation and physical loopback are different quantities
and must not be inferred from software CPU timings.

For the headless harness, file/direct-buffer transport can measure parser,
decoder/render and DSP CPU work but has no meaningful ALSA XRUN metric. An XRUN
or capture-queue-starvation assertion is valid only when an ALSA loopback or
physical device is actually participating. Only accepted captured-loopback
correlation may be called measured round-trip latency.

## Report acceptance

`aurora-sim latency-report` prints for every stage either a populated row with
`sample_count`, `p50_us`, `p99_us`, `max_us`, `overflow_samples`, or
`NOT_MEASURED`. It also prints a software-chain aggregate. Missing hardware stages
are never represented as zero latency.

A future physical acceptance result must add the capture and sink measurements,
name the exact head SHA and host, and demonstrate sustained scheduling without
XRUN/starvation before this document can contain a full-chain measured number.
