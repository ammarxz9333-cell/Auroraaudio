# Post-merge software validation — 2026-09-06

PRs #96, #97 and #98 were merged into main-v2. Audio source tested: `9a7347eed62ba6cd6fb8a8e513e5f75874f2ccfb`.

Files were fetched from that exact commit before testing.

- Optimized C broker build with -Wall -Wextra -Werror: passed.
- Real-pipe duplex-pressure regression: passed, 512 PCM periods with verified samples, bounded queue and retry checks.
- Real generated E-AC-3 in IEC61937 through the FFmpeg upmix adapter: passed, 49,152 frames of 12-channel PCM. Bed preservation, center/LFE/mono isolation, output before EOF and invalid-input rejection passed.
- Offline host throughput example: 10.016 seconds of synthetic audio processed in 0.166 seconds. This is not S6 timing or physical latency.
- Required Rust fmt/clippy/test/bench commands: blocked because Cargo/rustc are unavailable locally. The new side/back routing regression has NOT passed here.
- Full broker socket integration: blocked at AF_UNIX SOCK_SEQPACKET creation with EPERM.
- No workflow ran for the merge commit because S6 CI's push branch list omitted main-v2. This maintenance change adds main-v2; no assertions, required checks or failure handling are weakened.

No physical S6, HDMI/eARC, USB-MCU, speaker, live service, JOC object decoding or acoustic comparison has been validated by these tests. These results are partial software evidence, not a production readiness claim.
