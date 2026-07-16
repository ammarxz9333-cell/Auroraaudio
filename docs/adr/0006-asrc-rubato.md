# ADR 0006: Rubato Windowed-Sinc ASRC Adapter

- Status: Accepted for live-duplex validation
- Date: 2026-07-16

Use Rubato 0.16.2 behind Aurora's `AsynchronousResampler` trait. It is MIT
licensed, supports Rust 1.61 and therefore Aurora's Rust 1.78 baseline, provides
ramped ratio updates, fixed-output asynchronous sinc resampling, explicit delay,
and allocation-free `process_into_buffer` operation.

Aurora keeps planar conversion scratch and interleaved output cache preallocated.
No Rubato type crosses the public contract. A local scalar sinc implementation was
rejected because filter design, edge handling, SIMD selection, and long-term
maintenance carry greater correctness risk.

