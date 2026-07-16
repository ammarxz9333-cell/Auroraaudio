# Windows Backend Timing Model

## Scheduling Boundary

Aurora asks CPAL for a fixed block size, but the Windows shared-mode host may
invoke the callback with a different number of frames. The request is therefore
recorded as `requested_block_size`, not a negotiated device period. The callback
slice length is the only observed host callback size available to Aurora.

On the currently available display-audio output, a 256-frame request at 48 kHz
produced approximately 100 host callbacks per second with roughly 480 frames per
callback. Aurora processed each borrowed host buffer as configured-size chunks
plus a final tail. This is an observed callback shape, not a device-period or
latency measurement.

## Known Timing Values

- Requested processing block: supplied by Aurora configuration.
- Observed host callback frames: derived from each callback slice.
- Aurora processing time: measured around Aurora's processing call.
- Renderer and DSP algorithmic latency: reported by Aurora-owned processors.
- Software latency estimate: may combine configured buffering and algorithmic
  latency, but must remain labeled estimated.

## Unknown Values

CPAL 0.15 does not expose the WASAPI endpoint period, endpoint GUID, engine
buffer latency, hardware FIFO latency, or presentation position through Aurora's
current backend boundary. `device_period_frames` and
`device_reported_latency_frames` are therefore `None`. Driver, Windows Audio
Engine, USB/HDMI transport, converter, and acoustic latency remain unknown.

No value in this document is a measured round-trip latency. Round-trip latency
requires simultaneously emitted reference audio and physically captured input.

Synthetic duplex transport threads are intentionally labeled unpinned and are
not backend timing measurements. Their p50/p95/max values characterize software
transport only; Windows scheduling tails do not establish a device period.

Live duplex reports requested rate, CPAL-accepted input rate, CPAL-accepted output
rate, and adaptive output/input ratio separately. CPAL still exposes no endpoint
period, presentation position, or reliable Windows endpoint GUID through this
backend. Adaptive ring-fill correction is not a device-latency measurement.
