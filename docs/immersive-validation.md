# Immersive software validation

Aurora's real JOC validation lives in `validation/immersive/` and runs on a normal Linux host.

The baseline gate pins the external Harletty and Omniphony revisions from `config/external-components-v1.json`, runs Harletty's real JOC golden fixture, wraps the encoded stream in IEC61937, checks a plain E-AC-3 negative control, verifies object metadata/events/channel declarations through the bridge, and renders a 12-channel 7.1.4 raw-f32 output through Omniphony.

The realtime soak wrapper, `validation/immersive/test-joc-realtime-soak.sh`, first runs that baseline proof and then adds two continuous-stream checks:

- a repeated IEC61937 carrier is passed through the same Harletty bridge and must preserve every expected JOC packet/frame plus object metadata across repetitions;
- the original IEC61937 bursts are fed to Omniphony over stdin at their derived media cadence instead of being read as fast as possible. The 7.1.4 renderer must consume the stream without hanging, produce exactly the expected frame count, remain finite and non-silent, and finish close enough to media time to demonstrate that it kept up with the paced software input.

The paced cadence is derived from the already-validated baseline carrier and 7.1.4 output rather than hard-coding a device transport rate. CI currently repeats the fixture four times, which is about six seconds of continuous media time.

Baseline success requires:

```text
PLAIN-EAC3-NEGATIVE-CONTROL-PASS
JOC-IEC61937-PASS
7.1.4 render PASS
AURORA JOC SOFTWARE STACK PASS
```

The realtime soak additionally requires:

```text
JOC-CONTINUOUS-BRIDGE-PASS
JOC-PACED-7.1.4-PASS
AURORA JOC PACED STREAM PASS
```

Aurora's own realtime engine separately exposes callback counts, input/output underrun counters, dropped-block counts, callback duration and p95 timing, block-budget usage, and estimated renderer/DSP/device latency. Those metrics and the allocation guards remain generic software-core evidence; they are not replaced by the JOC soak.

These tests are software evidence only. They do not prove streaming-service access, DRM handling, physical eARC capture, a particular DAC or amplifier, speakers, thermals, licensed Dolby certification, or measured end-to-end latency on final hardware.
