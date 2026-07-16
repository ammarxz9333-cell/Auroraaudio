# Live Duplex

The live path is:

```text
CPAL input callback -> contiguous SPSC frame ring -> adaptive ASRC
-> Aurora real-time engine -> CPAL output callback
```

Input and output are separate streams with independent clocks. The input callback
copies borrowed interleaved samples into bounded storage. The output callback
drains fixed ASRC cache blocks, passes captured input to `RealTimeEngine`, and
writes directly into the borrowed CPAL output slice. Neither callback allocates,
logs, blocks, accesses files, or launches processes.

Example:

```text
aurora duplex --input-device <selector> --output-device <selector> \
  --sample-rate 48000 --block-size 256 --channels 2
```

`--input-sample-rate` and `--output-sample-rate` can request differing device
rates. Unsupported f32/channel/rate combinations are rejected before playback.
The status line distinguishes requested rate, negotiated input/output rates, and
the adaptive ratio. It reports callback/frame counts, fill extrema, underflow,
overflow/dropped frames, ratio/ppm/saturation, callback timing, latency components,
health, state, and numeric faults.

Startup begins capture first and waits for eight requested blocks of fill before
starting output. The ring capacity is 32 blocks. Shutdown stops output then input.
Backend faults stop both streams. Optional restart attempts reopen only the exact
selectors supplied by the user; Aurora never switches devices silently.

`aurora duplex-soak` runs the same path for a bounded duration and writes JSON,
including process working-set growth on Windows. Hardware results are reported
only for the channels and devices actually opened.

