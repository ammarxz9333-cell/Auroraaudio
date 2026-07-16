# Profiling Aurora On Windows

Always profile optimized binaries. Debug timing is not representative of audio
callback performance.

## Benchmarks

Run the complete release benchmark suite:

```powershell
cargo bench --workspace
```

Run only the direct median/p95 baseline:

```powershell
cargo bench -p aurora-realtime-engine --bench baseline
```

Run Criterion measurements only:

```powershell
cargo bench -p aurora-realtime-engine --bench performance
```

Criterion stores detailed reports under `target/criterion`. Compare runs on the
same power plan, with the same CPU load, and with release artifacts built by the
same Rust toolchain.

## Sampling Profiles

`cargo flamegraph --release --bench performance` can work when a compatible
Windows profiler and symbols are available. If flamegraph setup is unavailable,
use Windows Performance Recorder/Analyzer (WPR/WPA), Visual Studio Performance
Profiler, or Windows Performance Toolkit CPU sampling against a release build.

Build a release CLI before attaching a profiler:

```powershell
cargo build -p aurora-cli --release --all-features
target\release\aurora-cli.exe realtime --output-device 0 --scene fixtures\scenes\stereo_circle.json --sample-rate 48000 --block-size 256 --test-signal silence --duration-seconds 60
```

Do not profile `target\debug` when drawing latency conclusions. Include Rust
symbols, sample the audio callback thread, and look for allocation routines,
locking primitives, formatting, OS file/process calls, and unexpectedly expensive
math. Callback overruns appear when callback duration exceeds the block budget;
the CLI reports average, maximum, approximate p95, dropped blocks, and fault code.
