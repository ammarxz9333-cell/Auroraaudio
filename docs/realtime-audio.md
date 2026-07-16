# Real-Time Audio

Aurora's local path uses Aurora-owned device traits, a CPAL backend, and a
preallocated block engine. No CPAL type appears in Aurora public APIs.

## Pipeline

```text
CPAL output buffer
  -> live mono mapping or fixed-state test signal
  -> numeric RenderObject update
  -> caller-owned renderer gains and scratch
  -> preallocated planar routing
  -> in-place circular fractional delay
  -> interleaved output
  -> atomic metrics/status publication
```

All growable storage is allocated in `RealTimeEngine::new`. The renderer API uses
flattened caller-owned gain storage and index-based smoothing history. The delay
processor writes into caller-owned planar buffers. Capacity-invariance and a
thread-local counting-allocator test guard warmed-up steady-state processing.

Some Windows shared-mode devices deliver a host callback larger than the fixed
block requested through CPAL. The engine processes that borrowed output slice as
multiple configured-size sub-blocks plus a bounded tail; it does not resize its
storage or allocate. Callback count remains host callback count, while processed
block count reflects internal configured-size blocks.

## Metrics

The engine tracks callbacks, processed and dropped blocks, input/output
underruns, average/maximum duration, approximate p95 from a fixed 16-bucket
histogram, block budget and usage percentages, renderer/DSP/device latency, and a
fixed fault code. No metric is formatted on the callback thread.

The p95 value is an approximation bounded by histogram resolution. CPAL does not
currently expose device period or device-reported latency through this backend;
those values remain unknown rather than being replaced by the requested block.

## Limits

The CPAL backend currently opens f32 streams. Aurora exposes a descriptor-derived
selector while retaining run-local numeric indices for compatibility; CPAL does
not expose the Windows endpoint GUID, so name-based identity is best effort. The Windows
host stack and hardware driver remain outside Aurora's hard real-time guarantees.
The hardware-independent duplex bridge, live CLI orchestration, and deterministic
simulation backend are implemented. Live CPAL validation still requires suitable
local input and output hardware.
CamillaDSP remains offline and is never spawned by a callback.
