# Aurora i.MX93 R0 Software Compatibility Audit

Audit date: 2026-09-01

This document distinguishes executable software evidence from physical/BSP evidence. A green software gate does not prove a soldered i.MX93 carrier, SiI9437 tap, DAC clock tree, analog stage or amplifier safety.

## Selected software chain

```text
SAI1 S32_LE/192k/2ch IEC61937 capture
  -> aurora-iec61937-extract
  -> raw E-AC-3 access-unit byte stream
  -> Omniphony stdin Raw transport
  -> libharletty_bridge.so
  -> Harletty E-AC-3/JOC ObjectPcmDecoder + OAMD
  -> Omniphony VBAP 7.1.4
  -> PipeWire 12ch positioned stream
  -> aurora_tdm 16ch ALSA adapter
  -> hw:AuroraTDM16,0 S32_LE/48k/16ch
```

## Compatibility matrix

| Boundary | Software evidence | R0 result |
|---|---|---|
| SiI9437-style S32 capture -> Aurora IEC61937 | Aurora Rust tests cover high-word extraction, arbitrary chunk boundaries, E-AC-3 0x15 and Pd byte semantics | PASS (software) |
| Aurora extractor -> Omniphony stdin | Omniphony's decoder thread treats non-IEC61937 stdin chunks as `RInputTransport::Raw` and forwards them to the bridge | PASS (code audit + integration gate) |
| Raw E-AC-3 chunks -> Harletty | Harletty sniffs `0B77` and owns an incremental raw E-AC-3 extractor plus `ObjectPcmDecoder` | PASS (code audit + integration gate) |
| Harletty JOC/OAMD -> Omniphony | Harletty bridge is built against the exact sibling Omniphony `bridge_api`; the R0 build script enforces the pinned checkouts | PASS (build + integration gate) |
| Omniphony -> 12ch 7.1.4 file render | `validate-joc-714.sh` feeds Harletty's committed real E-AC-3 JOC fixture through the real bridge and renderer and verifies 12ch finite float output | REQUIRED CI GATE |
| 7.1.4 labels -> PipeWire positions | Aurora owns a layout using `TRL/TRR`; Omniphony resolves `TRL` as the same Top-Back semantic label and PipeWire SPA uses `TRL/TRR` | PASS (code audit) |
| 12ch renderer -> 16ch ALSA hardware node | Aurora PipeWire adapter fixes the hardware sink at S32LE/48k/16ch with 12 semantic positions + 4 AUX reserves | PASS (configuration); physical open is G6 |
| i.MX ASoC -> dual AK4458 | upstream NXP `fsl,imx-audio-card` supports two AK4458 codec DAIs, DSP_B TDM, 16ch TDM and TDM512; codec driver supports S32_LE and daisy-chain enable for >8ch | PASS (upstream Linux software); exact i.MX93 BSP/clock is G6 |
| service -> PipeWire session | R0 service is a systemd **user** unit and depends on the same user PipeWire/WirePlumber services | PASS (deployment model) |

## Pinned dependencies

R0 must use:

```text
Harletty  4ccedec804de3b29c02fb2a69575c2f49bf2fb37
Omniphony 44acc87a9cbf4b5ac8f474f51d87851d2c642550
```

`scripts/imx93/build-r0-deps.sh` checks these exact HEADs after checkout and fails on a mismatch.

## Cross-project integration test

The committed Harletty tree contains a genuine one-second E-AC-3 JOC regression fixture:

```text
harletty/tests/fixtures/joc_atmos_1s.eac3
```

Aurora's integration test runs it through the same post-IEC61937 software path used live:

```text
cat joc_atmos_1s.eac3
 -> orender stdin
 -> pinned libharletty_bridge
 -> pinned Omniphony renderer
 -> Aurora 7.1.4 layout
 -> raw float32 file
```

PASS requires:

- process exits successfully;
- output is non-empty;
- byte count is an integer number of 12-channel float32 frames;
- duration is plausible for the one-second fixture;
- every sample is finite;
- output is not silent.

The GitHub workflow `.github/workflows/imx93-r0-integration.yml` rebuilds the pinned third parties and executes this test on PRs that touch the R0 software path.

## Channel-order contract

The only accepted logical order is:

```text
FL FR C LFE BL BR SL SR TFL TFR TRL TRR
```

PipeWire positions become:

```text
FL FR FC LFE RL RR SL SR TFL TFR TRL TRR AUX0 AUX1 AUX2 AUX3
```

See `CHANNEL_MAP.md` for the TDM/DAC/power-stage mapping.

## Runtime/deployment checks

`doctor.sh` verifies:

- required host commands;
- Aurora extractor, Harletty bridge, Omniphony binary and layout exist;
- pinned dependency Git HEADs when source checkouts are present;
- exact 7.1.4 order;
- ALSA capture/playback inventory;
- `aurora_tdm` exists in the current PipeWire graph;
- user PipeWire/WirePlumber state;
- safe capture-open test;
- optional digital-zero 16ch playback-open test, disabled unless amplifiers are hard-muted.

`run-live-714.sh` fails closed if `aurora_tdm` is absent; Omniphony is therefore not allowed to silently fall back to a stereo/default sink.

## What software cannot close

These remain physical/BSP gates and must not be described as proven by CI:

1. **G2** — actual i.MX93 SAI1 slave capture of SiI9437 clocks/data at the required carrier.
2. **G3** — actual Netflix/eARC capture confirms untouched JOC/OAMD survives the selected TV/extractor chain.
3. **G4** — real-time CPU/thermal margin on the exact i.MX93 module; x86 GitHub CI is not an A55 benchmark.
4. **G5** — moving-object/height behavior measured from a real captured source on target hardware.
5. **G6** — SAI3 clock tree, 16-slot TDM512, dual-AK4458 slot identity and no xruns on the exact BSP/carrier.
6. **G7** — analog levels, DC/noise, startup mute and KAB9/PBTL safety.
7. **G8** — two-hour Netflix -> 7.1.4 soak and measured A/V sync.

## Current engineering verdict

Once the R0 integration workflow is green, no known **software-interface contradiction** remains in the selected chain. The remaining blockers are target-performance, BSP/device-tree realization and physical audio/electrical validation, not missing glue between Aurora IEC61937, Harletty, Omniphony and the intended Linux TDM output model.
