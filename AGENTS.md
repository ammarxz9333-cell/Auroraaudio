# Agent Instructions

- Keep Aurora hardware-agnostic. Core crates must not depend on a named phone, SBC, MCU, HDMI/eARC board, DAC, amplifier, speaker product, boot image, or appliance filesystem.
- Put optional platform integrations behind narrow adapter boundaries; they must be removable without changing the core audio model or renderer/DSP APIs.
- Never implement Dolby trademarked or patented codec behavior without explicit legal review.
- Never add DRM circumvention or protected-media extraction code.
- Never copy source from repositories with incompatible licenses.
- Keep third-party decoders, renderers, and DSP engines behind adapters with explicit version/license tracking.
- Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-features` before completing a software change.
- Update architecture documentation when changing public interfaces.
- Keep callback-reachable code allocation-free, lock-free, free of formatting/logging, and free of filesystem or process access.
- Use caller-owned fixed buffers for renderer and DSP steady-state processing; add allocation and capacity guards when changing these paths.
- Run relevant release-mode benchmarks after performance-sensitive realtime changes.
- Never label configured, estimated, timestamp-derived, simulated, or synthetic latency as measured latency. Measured round-trip latency requires accepted physical loopback capture.
- Keep accelerated simulation deterministic for equal seeds and allocation-free after scheduler startup.
- Live callbacks must not allocate, block, log, access files/processes, or silently change selected devices.
- Keep object decoding, channel decoding, and synthetic upmixing as distinct modes. Never silently fall back from one to another.
- Software validation proves software behavior only. Do not turn CI evidence into claims about physical eARC, USB, DAC, amplifiers, speakers, thermals, or wireless links.
- Prefer small, reviewable commits and preserve reproducible validation evidence.
