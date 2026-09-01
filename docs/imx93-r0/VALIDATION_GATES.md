# Aurora i.MX93 R0 Validation Gates

A successful build is not equivalent to a validated home-theater appliance. R0 uses explicit evidence gates so a software pass cannot accidentally become a hardware claim.

## Gate table

| Gate | Requirement | Current evidence | Pass condition |
| --- | --- | --- | --- |
| G1 | IEC 61937 E-AC-3 extraction | local Rust implementation | synthetic `0x15` payload round-trips byte-identically across arbitrary chunk boundaries |
| G2 | SiI9437 → i.MX93 SAI1 | vendor-supported + external SiI9437 hardware evidence | 10-minute 192 kHz S32 capture, zero xruns, stable BCLK/LRCLK, repeated valid Pa/Pb |
| G3 | Netflix JOC preservation | DD+ transport externally proven; JOC on exact chain unverified | known Netflix Atmos title yields `Pc=0x15` and Harletty reports JOC/object metadata from the captured stream |
| G4 | i.MX93 real-time decode | estimated from other AArch64 hardware | worst observed DD+ JOC average RTF <0.80 for 30 min; no deadline miss |
| G5 | Omniphony 7.1.4 | upstream speaker renderer exists | 12 distinct output channels with deterministic identification and correct object movement |
| G6 | SAI3 TDM512 / dual AK4458 | vendor-supported | 16 independent test slots reproduced at correct DAC outputs with common clock and no slip |
| G7 | analog + KAB9 | component-supported | level/noise/channel/mute/thermal checks pass on all twelve assigned outputs |
| G8 | complete appliance | unverified | 2-hour Netflix Atmos run, no underrun/channel swap/transient, acceptable lip sync |

## G1 software framing test

Run:

```bash
cargo test -p aurora-audio-io --bin aurora-iec61937-extract
```

The tests cover:

- E-AC-3 `Pd` byte units;
- AC-3 `Pd` bit units;
- arbitrary one-byte input chunking;
- the SiI9437 S32 high-word representation;
- codec filtering.

## G2 capture evidence

Required artifacts:

- `arecord --dump-hw-params` output;
- 10-second raw capture with SHA-256;
- measured BCLK and LRCLK frequencies;
- extractor count of valid E-AC-3 bursts;
- zero ALSA xruns in a 10-minute capture.

Reject G2 if success requires `plughw`, resampling or sample-format conversion. The encoded path must be bit preserving.

## G3 JOC evidence

A `Pc=0x15` header proves E-AC-3/DD+, not Atmos objects by itself. G3 requires decoder-level JOC evidence.

For the exact captured stream record:

- source title / playback device / TV audio mode;
- raw capture SHA-256;
- extracted `.eac3` SHA-256;
- Harletty codec/JOC report;
- object count and OAMD presence where exposed.

A 5.1 E-AC-3 core with no JOC does **not** pass G3.

## G4 performance evidence

Run on the exact byteENGINE module with CPU governor pinned for repeatability. Record:

- kernel/BSP version;
- CPU clock during run;
- Harletty commit;
- Omniphony commit;
- stream hash;
- audio seconds;
- user/system CPU seconds;
- RTF mean and worst-window value;
- xrun/deadline counters.

Acceptance target `RTF < 0.80` is intentionally stricter than `RTF < 1.0` because an appliance needs headroom for scheduling, I/O and transient blocks.

## G6 DAC transport evidence

Before power amplifiers are enabled, send one sine/test marker per slot through a 16-channel TDM test file. Verify all sixteen analog DAC outputs in order. Specifically prove the AK4458 daisy-chain split rather than inferring it from the datasheet.

## G7 amplifier safety evidence

Pass only when:

- 0 dBFS test cannot cause uncontrolled clipping at the selected gain structure;
- DC offset and startup/shutdown transient are acceptable;
- idle noise is acceptable at the listening position;
- channel identity remains correct after reboot;
- subwoofer PBTL configuration, if used, is independently validated;
- thermal behavior is stable for the intended enclosure.

## Final acceptance artifact

When G8 passes, create `docs/imx93-r0/evidence/R0_ACCEPTANCE_<date>.md` containing all exact versions, hashes, measurements and known limitations. Only that document is allowed to change the project wording from `hardware-integration target` to `validated R0 appliance`.
