# Channel surround upmix

`validation/surround-upmix/aurora-surround-upmix.sh` is an explicitly channel-based experimental path.

It decodes AC-3/E-AC-3 from IEC61937 with FFmpeg, preserves the decoded bed, and derives quiet decorrelated height ambience. It **does not** decode JOC/OAMD objects and must never be reported as Atmos/object recovery.

Run its software test with:

```bash
python3 validation/surround-upmix/test_surround_upmix.py
```

The test verifies bed preservation, synthetic height activity for suitable material, dialogue/LFE isolation, output before EOF, invalid-input rejection, and host-side throughput. Throughput is not a physical latency measurement.
