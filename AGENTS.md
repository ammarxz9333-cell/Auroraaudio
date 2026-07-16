# Agent Instructions

- Never implement Dolby trademarked or patented codec behavior without explicit legal review.
- Never copy source from repositories with incompatible licenses.
- Keep all third-party renderers behind adapters.
- Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` before completing a task.
- Update architecture documentation when changing public interfaces.
- Keep callback-reachable code allocation-free, lock-free, free of formatting/logging, and free of filesystem or process access.
- Use caller-owned fixed buffers for renderer and DSP steady-state processing; add allocation and capacity guards when changing these paths.
- Run `cargo bench --workspace` after performance-sensitive real-time changes and report release-mode results.
- Never label configured, estimated, timestamp-derived, or synthetic latency as measured latency; measured round-trip latency requires accepted physical capture.
- Treat sample-slip drift correction as proof-of-concept only, never as production asynchronous sample-rate conversion.
- Never label synthetic correlation, software buffering, or device-reported estimates as measured latency; measured round-trip values require captured physical loopback audio.
- Label virtual-device and virtual-loopback outputs as simulated truth, never measured hardware results.
- Keep accelerated simulation deterministic for equal seeds and allocation-free after scheduler startup.
- Live callbacks must not allocate, block, log, access files/processes, or silently change selected devices.
- Prefer small, reviewable commits.
- Do not add network or hardware functionality until Phase 0 acceptance tests pass.
- Do not add HDMI, ARC, eARC, HDCP, streaming-service capture, wireless speaker streaming, Raspberry Pi deployment, or mobile application functionality during Phase 0.
