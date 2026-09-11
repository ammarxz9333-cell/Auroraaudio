# Realtime health acceptance

Aurora evaluates realtime software runs in the control plane after metrics have already been collected by the realtime engine. Acceptance/report construction is deliberately outside the audio callback and may allocate.

## Default acceptance policy

The default policy requires:

- at least one callback;
- no persistent realtime engine fault;
- zero input underruns;
- zero output underruns;
- zero dropped blocks;
- p95 callback duration no greater than 100% of the configured block-duration budget.

An optional maximum for `estimated_end_to_end_latency_frames` can also be supplied. This value is an estimate derived from Aurora's software/device-buffering model. It is **not** a physical end-to-end latency measurement.

## CLI

`aurora realtime` prints a final `realtime_health=PASS` or `realtime_health=FAIL` verdict. A machine-readable report can be written with `--health-report`:

```text
cargo run -p aurora-cli --bin aurora-cli -- realtime \
  --scene fixtures/scenes/stereo_circle.json \
  --duration-seconds 30 \
  --health-report output/realtime-health.json \
  --max-p95-budget-percent 80
```

An estimated latency threshold can be added with `--max-estimated-latency-frames <frames>`.

When acceptance fails, Aurora writes the requested report first and then returns a command error. This preserves the failure evidence for CI or later inspection.

## Sustained software soak

The `Sustained Realtime Health Soak` workflow runs the real `RealTimeEngine` against the repository's generic 7.1.4 reference scene with a rotating test source. The run is accelerated rather than wall-clock paced so CI can validate a longer media interval efficiently without requiring an audio device.

The default CI gate simulates 600 seconds of 48 kHz media with 256-frame callbacks: exactly 112,500 callbacks. It requires:

- all callbacks to return `ProcessStatus::Ok`;
- exactly 12 output channels from the 7.1.4 scene;
- finite and non-silent sampled output throughout the run;
- zero input/output underruns and zero dropped blocks;
- no persistent realtime engine fault;
- strict realtime health acceptance to pass;
- a schema-v1 JSON health report whose counters match the expected callback count.

The workflow uploads the JSON report and execution log as bounded CI evidence. Because the run is accelerated and uses no physical audio endpoint, it proves sustained software-path stability only; it is not a physical timing or device-driver soak.

## Report contract

Schema version 1 is owned by the `aurora-realtime-acceptance` crate through `RealTimeHealthReportV1`. The CLI consumes this shared contract rather than defining its own JSON layout.

The corresponding JSON Schema is `schemas/realtime-health-report-v1.schema.json`. The report includes:

- the schema version and final acceptance verdict;
- observed p95 callback budget usage;
- the exact policy used for the decision;
- the realtime metrics used by that decision;
- a list of acceptance violations.

Adding fields compatibly may retain schema version 1. Removing fields, changing field meaning/type, or changing required-field semantics requires a new schema version.

## Scope

This gate is software validation. It does not establish hard-real-time guarantees, physical transport/DAC/amplifier/speaker validation, Dolby certification, DRM behavior, or compatibility with proprietary streaming services.
