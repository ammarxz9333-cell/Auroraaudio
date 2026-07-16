# Latency Budget

At 48 kHz, callback block budgets are:

| Frames | Budget |
| ---: | ---: |
| 64 | 1.333 ms |
| 128 | 2.667 ms |
| 256 | 5.333 ms |
| 512 | 10.667 ms |

Fixed algorithmic latency includes renderer latency and maximum configured DSP
delay. The basic renderer reports zero frames. Geometric delay reports its
maximum delay rounded up to a frame. CPAL currently exposes neither the WASAPI
device period nor device-reported latency through Aurora's backend. A requested
block may appear in a software estimate, but is never labeled negotiated or
measured latency.

Block-midpoint trajectory updates introduce up to half a block of control-rate
position quantization but do not buffer PCM. Host scheduling and OS mixer latency
are device dependent and are not measured by the current backend.

The target remains average processing below 25% and maximum processing below 75%
of the block duration. The release baseline bench prints median and p95 time,
budget percentages, and a linear estimated sustainable channel count. That
channel estimate is directional only: hardware, cache effects, filters, and host
scheduling are not linear.

The selected duplex frame ring stayed below 0.1% of block budget at threaded p95
for all 2/6/8/12-channel and 64/128/256/512-frame benchmark cases. This is
transport CPU time, not an added or measured physical latency value.

Live software buffering is reported as ring target plus renderer, DSP, and ASRC
algorithmic latency. It remains an estimate. Only accepted captured loopback
correlation is reported as measured round-trip latency.
