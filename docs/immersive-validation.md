# Immersive software validation

Aurora's real JOC validation lives in `validation/immersive/` and runs on a normal Linux host.

The gate pins the external Harletty and Omniphony revisions from `config/external-components-v1.json`, runs Harletty's real JOC golden fixture, wraps the encoded stream in IEC61937, checks a plain E-AC-3 negative control, verifies object metadata/events/channel declarations through the bridge, and renders a 12-channel 7.1.4 raw-f32 output through Omniphony.

Success requires all markers:

```text
PLAIN-EAC3-NEGATIVE-CONTROL-PASS
JOC-IEC61937-PASS
7.1.4 render PASS
AURORA JOC SOFTWARE STACK PASS
```

This is software evidence only. It does not prove streaming-service access, DRM handling, eARC capture hardware, DAC output, amplifiers, speakers, thermals, or measured end-to-end latency.
