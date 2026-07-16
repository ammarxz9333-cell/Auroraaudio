# Real-Time Allocation Audit

This audit records the callback-reachable path before Optimization Sprint 1. It
is intentionally a description of the pre-optimization implementation. The ADR
and threading documentation describe the replacement design.

## Callback Call Graph

`aurora-cli::run_realtime` installs a CPAL output callback which calls
`RealTimeEngine::process_interleaved`. That function calls, in order:

1. `RealTimeEngine::fill_mono`
2. `RealTimeEngine::render_planar`
3. `BasicRenderer::update_objects`
4. `BasicRenderer::render_gains`
5. `BasicRenderer::gains_for_object` and `BasicRenderer::smooth_gain`
6. `DelayProcessor::process_block`
7. `interleave`
8. atomic metric stores in the CLI callback

The CPAL backend error callback only stores an `AtomicBool`.

## Heap Allocations Reachable From The Callback

| Function | Allocation |
| --- | --- |
| `RenderScene::object_at_time` / `rotating_object` | Creates an `AudioObject` containing an owned `String`. The rotating path calls `to_owned` every block. Scene trajectory sampling may clone the scene object identifier. |
| `BasicRenderer::update_objects` | Takes ownership of a newly constructed `Vec<AudioObject>` every block and replaces the previous vector. |
| `BasicRenderer::render_gains` | Clones the complete object vector and collects a new outer `Vec<Vec<SpeakerGain>>`. |
| `BasicRenderer::gains_for_object` | Collects enabled speaker references, raw gain vectors, and the final `Vec<SpeakerGain>`; every `SpeakerGain` clones a speaker identifier. |
| `nearest_speaker_gains`, `inverse_distance_gains`, `adjacent_speaker_gains`, `normalize_power` | Return newly allocated vectors. Adjacent mode also allocates and sorts a distance vector. |
| `BasicRenderer::smooth_gain` | Allocates two owned strings for every object/speaker key and inserts them into a `HashMap`; insertion can grow and rehash the map. |
| `DelayProcessor::process_block` | Allocates the outer output vector, clones each history channel, extends each clone, allocates every output channel, and pushes every sample. |
| error construction in `RealTimeEngine::render_planar` | Allocates a `String` when the renderer unexpectedly returns no gains. |

`RealTimeEngine::mono`, `planar`, and `delayed` are allocated during setup and do
not themselves grow in the callback. However, `delayed` is populated from newly
allocated DSP output, so it does not eliminate callback allocation.

## Locks

No `Mutex`, `RwLock`, condition variable, channel send, or other blocking lock
is reachable from the current output callback. CLI metric communication uses
relaxed atomics. CPAL owns platform-internal synchronization outside Aurora's
callback implementation; Aurora cannot prove the host implementation lock-free.

## String Formatting And Logging

There is no `println!`, `eprintln!`, tracing call, or explicit `format!` on the
successful callback path. Status formatting occurs on the control thread.
`thiserror` display formatting is not invoked by the callback. String allocation
still occurs through `to_owned`, cloned identifiers, and error payload creation.

## Filesystem And Process Access

No filesystem operation, process spawn, environment lookup, device enumeration,
or sleep is reachable from `process_interleaved`. Scene loading, CamillaDSP
process work, device opening, status sleep, and console output remain on the
control thread.

## Panic Paths

The callback does not contain an explicit `unwrap`, `expect`, `panic!`, or
assertion. It does contain bounds-indexing and arithmetic assumptions that can
panic:

- `output.len() / output_channels` divides by zero if a scene produces no roles.
- `fill_mono` indexes `self.mono[frame]` and slices input with calculated bounds.
- `fill_mono` impulse mode indexes `self.mono[0]` after frame validation.
- `render_planar` indexes `self.mono[frame]`, `self.planar[channel_index]`, and
  each channel at `frame`; a renderer/channel count mismatch panics.
- `interleave` uses unchecked-by-contract nested indexing into output and planar
  channels; malformed buffer lengths panic.
- `DelayProcessor::process_block` indexes delays/history by channel, indexes
  interpolation samples, and calls `copy_from_slice`; inconsistent internal
  lengths panic.
- `RingBuffer::push` indexes an empty vector when capacity is zero, although the
  ring buffer is not currently called by the audio callback.
- duration averaging casts nanoseconds from `u128` to `u64`; it does not panic,
  but can truncate after extreme runtimes.

Returned renderer/DSP errors are caught by the CLI callback, which fills silence
and increments `dropped_blocks`. The error values can own allocated strings, and
there is no persistent fault code to tell the control thread what failed.

## Buffer Ownership And Lifetimes

- CPAL owns the interleaved output slice for the callback duration.
- The callback closure exclusively owns `RealTimeEngine` for the stream lifetime.
- `RealTimeEngine` owns fixed setup-time mono, planar, delayed, renderer, delay,
  signal-state, and metric storage.
- `BasicRenderer` owns cloned scene speakers, listener state, object vectors, and
  a dynamically growing smoothing map.
- `DelayProcessor` owns per-channel history but constructs temporary extended and
  output buffers for every call.
- The control thread owns the stream handle and reads callback results only from
  atomic counters. No borrowed audio buffer crosses the callback boundary.

## Current Latency Sources

- Device buffering: reported conservatively as one negotiated callback block.
- Renderer: reports zero algorithmic frames, while block midpoint position
  updates add up to half a block of control-rate temporal quantization.
- Geometric delay: maximum configured per-speaker delay, rounded up to frames.
- Callback scheduling and host mixing: backend/OS dependent and not measured by
  the current CPAL wrapper.
- Allocation, cloning, sorting, hash lookup/insertion, and deallocation add
  nondeterministic callback execution time even though they add no fixed frames.
- The control thread samples status once per second; this affects observability,
  not audio latency.

## Hard Real-Time Violations To Remove

The exact violating functions are:

- `aurora_realtime_engine::RealTimeEngine::render_planar`
- `aurora_realtime_engine::rotating_object`
- `aurora_renderer_api::Renderer::update_objects`
- `aurora_renderer_api::Renderer::render_gains`
- `aurora_renderer_basic::BasicRenderer::gains_for_object`
- `aurora_renderer_basic::BasicRenderer::smooth_gain`
- `aurora_renderer_basic::nearest_speaker_gains`
- `aurora_renderer_basic::inverse_distance_gains`
- `aurora_renderer_basic::adjacent_speaker_gains`
- `aurora_renderer_basic::normalize_power`
- `aurora_dsp_basic::DelayProcessor::process_block`
- `aurora_realtime_engine::interleave`

The replacement must use caller-owned fixed buffers, index-based smoothing
state, in-place delay processing, non-allocating status codes, validated buffer
shapes, and atomically published metrics/faults.

## Sprint 1 Remediation

Optimization Sprint 1 replaced the violating callback path while retaining this
pre-change audit as evidence. `Renderer::render_gains` now writes to caller-owned
`SpeakerGain` and `RendererScratch` buffers; smoothing is index based.
`RealTimeEngine` samples trajectories into a stack `RenderObject`, owns fixed
mono/planar/delayed/gain/scratch storage, and calls
`DelayProcessor::process_block_into`, which uses circular history buffers.

The callback publishes `RealTimeFault` and timing counters without formatting.
Malformed input/output is silenced through checked paths. A thread-local counting
allocator test records zero allocations across 1,000 warmed-up blocks, and a
separate 10,000-block test verifies all callback buffer capacities are unchanged.
The counting allocator tracks allocations made on the measured test thread; it
does not observe allocations inside CPAL, the Windows audio service, drivers, or
other threads. It is therefore paired with API shape review and capacity guards,
not treated as proof about external host code.
