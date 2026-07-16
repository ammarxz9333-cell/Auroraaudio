# Physical Latency Validation

`aurora measure-latency` emits a deterministic repeated bipolar sequence while a
separate CPAL input stream captures real samples. After both streams stop, the
control thread performs normalized cross-correlation and rejects silent or
low-confidence captures. Callback code only writes preallocated capture storage.

Required setup:

1. Connect a physical analog cable from the selected output to the selected input,
   or use a hardware loopback path whose physical route is explicitly understood.
2. Select the exact input and output shown by `aurora devices`.
3. Set playback and capture gain low enough to avoid clipping and high enough for
   correlation.
4. Run:

```text
aurora measure-latency --input-device <selector> --output-device <selector> \
  --sample-rate 48000 --block-size 256 --duration-seconds 10
```

Only a non-silent captured signal with accepted correlation is labeled measured
round-trip latency. The report includes median/minimum/maximum samples, converted
milliseconds, jitter, confidence, and valid count. `--save-capture` writes the
captured first channel after the streams stop. Synthetic estimator tests are never
reported as physical measurements.

