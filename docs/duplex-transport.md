# Duplex Transport Benchmark

## Scope and Method

The release benchmark runs at 48 kHz for 2, 6, 8, and 12 interleaved channels
with 64, 128, 256, and 512 frame blocks. It compares sequential producer and
consumer calls with two separate Windows threads. The threaded run is unpinned:
adding a platform affinity dependency solely for a synthetic benchmark was not
justified, so scheduler migration remains a source of maximum-time noise.

Each case performs 1,000 measured round trips after fixed startup allocation.
The output reports producer and consumer p50/p95/max, aggregate sample
throughput, p95 percentage of one block period, and minimum uncontended atomic
operations. Atomic counts are source-level lower bounds; retries add operations.

Run:

```text
cargo bench -p aurora-realtime-engine --bench duplex_transport
```

## Compared Designs

| Design | Storage | Minimum atomics per round trip | Constraint |
| --- | --- | ---: | --- |
| `ArrayQueue<f32>` | one queue entry per sample | `8 * samples + 4` | atomics grow with channels and frames |
| fixed block pool | preallocated slots plus lengths | 8 per block | fixed block shape; awkward partial callbacks and drift edits |
| contiguous frame ring | preallocated interleaved samples | 6 per block | one producer and one consumer only |

All designs are bounded, lock-free in the callback path, preserve interleaved
frame order, and allocate zero times after startup. Third-party types do not
appear in Aurora public APIs.

## Representative Results

The selected contiguous frame ring produced the following threaded-unpinned
release results on the checkpoint Windows host. Times are per block and may vary
with scheduling. Throughput is aggregate producer plus consumer throughput.

| Channels | Frames | Producer p50/p95 | Consumer p50/p95 | Worst max | Msamples/s | p95 budget |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2 | 64 | 0.2/0.3 us | 0.2/0.3 us | 5.7 us | 811 | 0.023% |
| 2 | 128 | 0.4/0.4 us | 0.4/0.4 us | 6.6 us | 1,086 | 0.015% |
| 2 | 256 | 0.8/0.8 us | 0.8/0.8 us | 8.6 us | 1,165 | 0.015% |
| 2 | 512 | 1.4/1.4 us | 1.4/1.4 us | 4.5 us | 1,401 | 0.013% |
| 6 | 64 | 0.6/0.7 us | 0.6/0.7 us | 8.0 us | 1,123 | 0.053% |
| 6 | 128 | 1.1/1.2 us | 1.1/1.2 us | 4.8 us | 1,253 | 0.045% |
| 6 | 256 | 2.1/2.2 us | 2.1/2.2 us | 13.4 us | 1,416 | 0.041% |
| 6 | 512 | 4.0/4.0 us | 4.0/4.0 us | 36.7 us | 1,493 | 0.038% |
| 8 | 64 | 0.8/0.8 us | 0.8/0.8 us | 7.0 us | 1,181 | 0.060% |
| 8 | 128 | 1.4/1.5 us | 1.4/1.5 us | 8.8 us | 1,334 | 0.056% |
| 8 | 256 | 2.7/2.8 us | 2.7/2.8 us | 51.7 us | 1,385 | 0.053% |
| 8 | 512 | 5.3/5.5 us | 5.3/5.5 us | 47.4 us | 1,459 | 0.052% |
| 12 | 64 | 1.1/1.2 us | 1.1/1.2 us | 2.1 us | 1,300 | 0.090% |
| 12 | 128 | 2.0/2.1 us | 2.0/2.1 us | 8.7 us | 1,473 | 0.079% |
| 12 | 256 | 4.0/4.1 us | 4.0/4.1 us | 13.2 us | 1,500 | 0.077% |
| 12 | 512 | 7.9/8.1 us | 7.9/8.1 us | 96.1 us | 1,500 | 0.076% |

The sample queue reached a worst threaded p95 budget share of 1.575% and about
208 Msamples/s at 12 channels. The block pool was fastest, generally 1,245 to
5,332 Msamples/s, but does not naturally serve variable callback tails or
frame-level drift transitions. The contiguous ring reached roughly 811 to 1,500
Msamples/s and keeps those operations within one transport.
