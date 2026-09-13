# Aurora Intel Simics harness

Run from native PowerShell:

```powershell
& C:\Users\ammar\Auroraaudio-latest\validation\simics\run-simics.ps1
```

Requires the installed Simics 7.84.0 project at
`C:\Users\ammar\simics-projects\aurora`. The launcher accepts `-SimicsProject`,
`-PcmPath` and `-OutputDirectory`. Alternatively run that project's
`targets/aurora/aurora.simics` in batch mode. Both use the same repository sources.
The test exits Simics with status 0 only on complete success, 1 on assertion or
execution failure. The launcher checks the report run ID to reject stale evidence.

## Contract and limits

This is an **abstract Aurora audio I/O device**, implemented with Simics `pyobj`,
the installed vacuum target's 20 MHz clock, mapped MMIO and RAM DMA transactions.
It is not an STM32, USB controller, MCHStreamer or DAC register specification.
No guest CPU/OS/driver or live JOC decoder executes in vacuum. The complete saved
Aurora output-DSP PCM is replayed through the model. Timing is simulated, not
host pacing, physical timing, DMA bandwidth or acoustic latency evidence.
IRQ is a latched register; routing to a guest interrupt controller is unimplemented.
Checkpoint save/restore is unsupported (Python event/state is not serialized).

Fixed accepted format: 48 kHz, 12 interleaved float32 channels, 240-frame periods
(5 ms / 100,000 vacuum cycles). The two-entry FIFO captures address and length
on submit; RAM is read when the scheduled event fires. Reads use at most 1024
bytes per Simics memory-space transaction. Each frame is packed into 16 abstract
32-bit slots: unchanged channel words 0–11, zero padding 12–15. This preserves
float payloads; it does not claim a physical DAC accepts IEEE float words.

MMIO base `0x40000000`, 32-bit little endian, aligned transactions only:

| Offset | Register | Behavior |
| --- | --- | --- |
| 0x00 | CTRL | 0 stop and mute, 1 start, 2 reset |
| 0x04 | STATUS | Read-only: 0 idle, 1 running, 2 fault |
| 0x08 | SOURCE | RAM address, default 0x10000000 |
| 0x0c | LENGTH | Exactly 11520 bytes per descriptor |
| 0x10 | RATE | 48000 |
| 0x14 | CHANNELS | 12 |
| 0x18 | PERIOD | 240 frames |
| 0x1c | SUBMIT | Write 1 to queue descriptor |
| 0x20 | DONE | Read-only completed periods |
| 0x24 | IRQ | Bit 0 completion, bit 1 error; write-one-to-clear |
| 0x28 | ERROR | Read-only latched error code |

Errors: 1 unsupported format, 2 invalid control, 3 descriptor length/doorbell,
4 DMA alignment/range, 5 FIFO full, 6 live format mutation, 7 invalid register
access, 8 starvation, 9 memory transaction failure, 10 non-finite PCM.
All errors cancel future events, flush pending DMA, mute the output period and
latch until reset. Stop also cancels events and drops pending descriptors.

## Acceptance

`run_in_simics.py` tests no early completion, exact first deadline, IRQ W1C,
stop/reset event cancellation, read-only/width/alignment/unknown MMIO rejection,
invalid formats/descriptors, FIFO overrun and ordering, deferred RAM reads,
active-stream underrun, unmapped DMA RAM and non-finite data. Then it replays the
complete DSP PCM and compares frame count, SHA-256 identity and independently
constructed dual-TDM8 transport bytes. Every deadline is checked to within two vacuum cycles.

Current 2026-09-13 local result: **21 profiles PASS**, 3,624,960 frames / 15,104 periods,
75.52 simulated seconds, zero healthy-path xruns. The MCHStreamer contract models TDM16 as
two parallel TDM8 lanes (8 x 32-bit slots per lane), with channels 13-16 zero-padded.
UAC2 asynchronous feedback tests also cover +/-250 ppm device-clock offset. Full report and source
hashes: `artifacts/simics/aurora-simics.json`; console: `artifacts/simics/simics.log`.
The printed unmapped-memory diagnostic is an intentional negative test.

The TV-to-speakers extension adds an ideal DAC/amplifier/load test to the output
suite and a separate scheduled TV ingress model. See `TV-TO-SPEAKERS.md` for the
fresh encoded-source-to-load runner and its explicit Netflix acceptance boundary.

The main AuroraSim coverage entry remains planned for integration: this separate
Simics suite does not export a capability through `aurora_full_system_sim.py`.
Passing this suite does not promote vendor, guest-driver or physical coverage.

API reference: installed Simics 7.84.0 Python framework and
[Intel's device modeling overview](https://intel.github.io/simics/docs/model-builder-user-guide/device-modeling-overview.html).
