# Q995-Class Product Acceptance Matrix

This matrix is the product gate. A subsystem is not complete because a crate exists, a fixture exists, CI compiles, or a simulator passes. The listed evidence is mandatory for promotion.

| Area | Required product behavior | Current baseline | Software gate | Physical gate |
|---|---|---|---|---|
| TV input | Real eARC/ARC carrier ingest, clean lock/loss/relock | S6/MCU portable path host-tested | IEC61937 framing, discontinuity, reconnect and source-gate tests | Real TV + eARC front-end capture across 48/96/192 kHz carriers |
| Immersive input | Stable object/height-capable 7.1.4 program path | External Harletty/Omniphony staged | Legal sample, deterministic channel/layout evidence, bounded realtime handoff | Real streaming source, measured object/height/channel behavior, long soak |
| Channel transport | Correct 12-channel 48 kHz transport | Host-tested framing/protocol | layout hash, sequence/PTS, queue/XRUN/reset tests | USB/TDM timing, no channel swaps, sustained xrun test |
| Horizontal rendering | Smooth front/side/rear panning | 2D VBAP implemented | deterministic probe/motion artifacts | acoustic channel-identification and motion listening/measurement |
| Height rendering | Genuine 3D 5.1.2 and 7.1.4 panning | Not accepted in current main | 3D triplet renderer, overhead probes, energy/continuity artifacts | up-firing/direct-height acoustic validation in target enclosure/room |
| Binaural | Headphone spatial reference | geometric ITD/ILD only | honest capability report | optional listening validation; not a soundbar release blocker |
| Per-channel timing | Speaker distance/delay alignment | fractional delay exists | bounded delay, dynamic update/discontinuity tests | microphone time-of-flight verification |
| Bass management | Main/sub crossover and LFE policy | missing | crossover response tests, headroom/sum policy, deterministic filters | measured main/sub integration and seat response |
| EQ/room correction | Per-channel target response | missing product graph | PEQ/FIR coefficient validation and response artifacts | calibrated mic measurements before/after in multiple rooms |
| Protection | No uncontrolled clipping/overdrive | missing product limiter | limiter knee/attack/release/headroom tests | amplifier/speaker high-level verification |
| Calibration | Automatic identity, polarity, delay, level, response | measurement foundations only | deterministic estimator fixtures and confidence/error outputs | repeated mic captures, known-distance/polarity/reference-room error bounds |
| Source manager | One final audible owner, safe handoff | host validated | grant/revoke/quiesce/watchdog/ACK tests | real HDMI/local/BT handoffs without pops or stale audio |
| Local music | Consumer playback into same final DSP | source contract only | production adapter and source-manager integration | real playback/source switch soak |
| Bluetooth | Stable source integration | packages staged; adapter missing | production adapter, reconnect and codec-boundary tests | RF/reconnect/use-case validation |
| Rear transport | Synchronized wireless rear channels | design concept only | packet/jitter/clock/recovery simulator and receiver implementation | measured inter-node skew, loss recovery and RF soak |
| Multiroom | Grouped synchronized program playback | missing | multi-process/group/join/leave tests | multi-room clock/skew/network validation |
| UI | Settings, source, status, now playing, calibration | missing LVGL app | UI state-machine and control-contract tests | real S6 display/touch usability and recovery |
| Boot/update | Reproducible appliance and safe recovery | build scripts exist | reproducible image/manifests/update rollback | actual S6 boot/flash/recovery/update testing |
| Thermal | Sustained full processing without throttle/xrun failure | unmeasured | CPU budget instrumentation | enclosure thermal soak with immersive program |
| Lip-sync | Stable A/V sync across source changes | control state exists | propagation and applied-DSP offset tests | camera/audio or equivalent measured A/V latency |
| Fault recovery | Fail closed, recover without corrupt state | strong host foundations | USB/source/network/process fault injection | cable/power/RF/source-loss physical fault campaign |

## Promotion rule

A row can be promoted to `production_ready` only when both its required software gate and physical gate pass on the selected product hardware. Dependencies may have stronger evidence, but they do not automatically promote dependent rows.

## Release-critical rows

For the first Q995-class theater release, the following are release blockers: TV input, immersive input, channel transport, horizontal rendering, height rendering, per-channel timing, bass management, EQ/room correction, protection, calibration, source manager, rear transport, UI, boot/recovery, thermal, lip-sync and fault recovery.

Local music, Bluetooth and multiroom may be feature-gated from the first release if their capability registry state and UI accurately report them as unavailable; they must not destabilize the theater path.
