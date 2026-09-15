# Prepared binaural PCM

`aurora_renderer_basic::binaural` is a prepared stereo PCM processor. Inputs are explicit stable mono-object channels (1–16) or full ACN/SN3D Ambisonics orders 1–3. An immutable `Filters` bank contains input-major, ear-minor FIR responses. Other normalizations and partial HOA sets are unsupported.

Control flow is `Filters::prepare -> PreparedBinaural::new -> commit -> process -> discontinuity`. Preparation validates dimensions, rates, finite coefficients and each response's L1 bound. Commit requires matching input/rate/tap contracts and a strictly newer generation. A busy transition rejects another commit without changing state. Ownership and exclusive mutable access prevent concurrent filter mutation; callers must not wrap callback access in a blocking lock.

Processing consumes normalized interleaved PCM, maintains fixed input history and emits stereo. Linear sample crossfades share history and preserve tails across block boundaries. Invalid input silences the output and leaves history unchanged; explicit discontinuity clears history and selects any already-accepted target bank. No callback storage allocation, release, process/file access or formatting occurs. Filter onset/group delay is separate from the zero extra buffering latency. Output may exceed unity; downstream gain/headroom handling is required. Work scales with channels × taps × frames; fixed bounds are not a universal deadline guarantee.

## Reference evidence

The isolated modern-Rust exporter uses exact sofar `06a629292689e99841e5dacaa25c4c6298616ca6`, libmysofa `da9e4adc619ee3d1ae5e68da3ed14aa5e60b3ec1`, and the MIT KEMAR dataset attributed to Bill Gardner and Keith Martin (see THIRD_PARTY_LICENSES.md). It embeds SOFA delays by fractional sample interpolation before comparing native convolution against sofar partitioned convolution on identical filters.

The 19 cases include object front/back/left/right/up/down, yaw/pitch/roll prepared poses, and directional order-1/2/3 HOA fields. HOA filters use an 8 × 16 spherical quadrature and SN3D dual-basis degree factors. This is a finite spatial approximation, not arbitrary HRTF-field equality. OBR `478dc7c752d5eccae534635139ff0253eee3a14a` checks directional semantics independently. Its HOA rotation comparison uses an equivalent head-coordinate plane wave; it does not claim arbitrary live HOA rotation testing.

Native PCM equality is bounded by 1e-5 absolute sample error. Front/back and elevation distances indicate different transfer responses, not listener perception. Fault controls reject silent output, ear swaps and missing cases. Contract tests exercise impulse tails, channel isolation, partition-independent transitions, invalid/stale/busy candidates, and allocation-free processing/recovery.

## Remaining integration

This API is implemented and executable, but general application/runtime component selection, sustained deadline profiles and moving-source transition metrics remain separate unfinished gates. There is no physical eARC, DAC, acoustic, tracker, protected-service, perceptual-quality or certification claim.
