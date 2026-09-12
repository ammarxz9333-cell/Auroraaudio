# Aurora full-system virtual hardware lab

Issue: #145

## Purpose

This validation lane exercises Aurora's already-proven moving-JOC software path and extends its 12-channel 48 kHz output through a deterministic laptop-only virtual hardware sink.

The healthy path is:

`checksum-pinned Dolby-derived JOC -> IEC61937 0x15 -> pinned Harletty -> pinned Omniphony 7.1.4 -> paced 12ch f32 -> virtual synchronous TDM16/DAC sink -> evidence report`

The virtual hardware model is intentionally narrow. It preserves Aurora's 12 rendered channel positions into TDM slots 0..11, holds slots 12..15 at digital zero, keeps one synchronous virtual clock domain, and records deterministic source/sink/TDM hashes.

## Run

The complete gate is:

```bash
bash validation/virtual-hardware/test-aurora-full-system-sim.sh /tmp/aurora-full-system-sim
```

It reuses `validation/immersive/test-joc-aurora-moving.sh`; the JOC carrier, bridge, object metadata, 7.1.4 render and pacing are therefore the same evidence path used by the Aurora moving-JOC gate rather than a second synthetic decoder path.

The virtual hardware analyzer can also be exercised independently:

```bash
python3 validation/virtual-hardware/aurora_full_system_sim.py self-test
```

## Healthy acceptance

The healthy profile requires:

- upstream moving-JOC evidence verdict `pass`;
- 12 rendered channels at 48 kHz;
- exact source frame count from the JOC evidence;
- exact source-to-virtual-sink frame preservation;
- all 12 virtual output channels contain activity;
- no non-finite samples;
- no virtual xruns;
- no virtual disconnect;
- one 16-slot synchronous virtual transport;
- slots 0..11 preserve channel order and slots 12..15 remain unused/zero;
- deterministic SHA-256 evidence for source PCM, virtual sink PCM and TDM16 stream.

The default configured virtual transport latency is 256 frames (5.333 ms at 48 kHz). It is **simulated**, not measured.

## Fail-closed negative profiles

The harness also injects deterministic failures and requires every one to fail:

- `dropout`: periodically removes an output frame and records virtual xruns;
- `channel-silence`: forces one output channel inactive;
- `disconnect`: terminates the virtual sink halfway through the stream;
- `drift`: injects +250 ppm virtual output-clock drift, above the healthy limit.

A negative profile returning a healthy verdict fails CI.

## Evidence

The healthy report is written to:

`virtual-hardware/aurora-full-system-sim.json`

Negative reports are written beside it as `fault-*.json`.

The dedicated workflow `.github/workflows/aurora-full-system-sim-ci.yml` uploads the virtual reports together with the upstream moving-JOC evidence used by the run.

## Truth boundary

A green result proves the tested Aurora software path plus the deterministic virtual output model. It does **not** prove:

- physical eARC capture;
- USB/UAC2 enumeration or electrical timing;
- physical TDM signal integrity;
- DAC, amplifier or loudspeaker behavior;
- physically measured latency or clock drift;
- Netflix/other DRM-service compatibility;
- Dolby certification/conformance;
- acoustic parity with a commercial sound system.

Those remain separate physical acceptance gates under issue #143.
