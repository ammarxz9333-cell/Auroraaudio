# Aurora Project Execution State

## Active target

Aurora is now executing **Aurora i.MX93 R0 — Private 7.1.4 Appliance** on branch `aurora-imx93-r0`.

The purpose of R0 is one physical, private-use system that can accept a TV eARC DD+ Atmos/JOC stream and render it to a real 7.1.4 loudspeaker array. The existing Rust platform remains useful infrastructure, but the R0 hardware path and its acceptance gates take precedence on this branch.

## Selected architecture

The selected path is:

```text
TV eARC
→ Lindy 38368 / SiI9437
→ SAI1 RX on byteENGINE i.MX93 OSM-S
→ IEC 61937 deframe
→ Harletty E-AC-3 JOC bridge
→ Omniphony 7.1.4
→ SAI3 TDM512
→ 2 × AK4458 in daisy-chain mode
→ analog conditioning
→ 2 × KAB9
→ 7.1.4 speakers
```

### Locked R0 decisions

- Main compute module: byteENGINE i.MX93 OSM-S, dual Cortex-A55 1.7 GHz class.
- eARC prototype front end: Lindy 38368 containing Lattice SiI9437.
- Protected video is not captured by Aurora; the TV remains the HDCP endpoint.
- Encoded eARC ingress: SiI9437 I2S tap, not optical S/PDIF and not HDMI video capture.
- Input serial port: SAI1 RX as external-clock slave.
- DD+ carrier: 2-channel S32_LE at 192 kHz for the validated SiI9437 representation.
- IEC 61937: Aurora-owned deframer, preserving E-AC-3/JOC for Harletty.
- Object decoder: Harletty bridge pinned to commit `4ccedec804de3b29c02fb2a69575c2f49bf2fb37` for R0 validation.
- Object renderer: Omniphony pinned to commit `44acc87a9cbf4b5ac8f474f51d87851d2c642550` for R0 validation.
- Physical render layout: 7.1.4 at 48 kHz.
- Native DAC bus: SAI3 TDM512, 16 × 32-bit slots.
- DAC stage: two AK4458 devices using the manufacturer's TDM512 daisy-chain architecture.
- Amplifier reference: two WONDOM KAB9 boards; 12 channels are assigned, four DAC channels remain reserve.

## Evidence classification

Use exactly these terms:

- **proven-local** — reproduced by Aurora code or hardware under our control;
- **proven-external** — reproduced by a credible external hardware project or vendor reference;
- **supported** — documented by the component/vendor but not yet exercised in Aurora;
- **estimated** — performance or behavior inferred from measurements on another platform;
- **unverified** — no acceptable physical evidence yet.

Do not convert `supported` or `estimated` into `proven` in README text, issue titles or commit messages.

## R0 acceptance gates

1. **G1 — software framing:** synthetic IEC 61937 `0x15` burst survives arbitrary chunking and extracts byte-identical E-AC-3. Implemented locally.
2. **G2 — SAI1 electrical capture:** i.MX93 captures SiI9437 BCLK/LRCLK/SD0 without xruns or bit errors at the DD+ 192 kHz carrier.
3. **G3 — real stream identity:** a known Netflix Atmos title produces IEC 61937 `Pc=0x15` and the extracted E-AC-3 is identified by Harletty as JOC with nonzero object metadata.
4. **G4 — real-time decode:** worst observed DD+ JOC stream stays below `RTF 0.80` average on the exact production-clocked i.MX93, with no frame deadline misses during a 30-minute run. The 0.80 threshold deliberately reserves system headroom.
5. **G5 — 7.1.4 render:** Omniphony emits twelve correctly ordered speaker channels from the captured object stream.
6. **G6 — TDM512 transport:** SAI3 emits stable 48 kHz / 32-bit / 16-slot TDM512; both AK4458 devices lock and all sixteen DAC outputs can be identified independently.
7. **G7 — analog/amplifier:** twelve assigned outputs meet level, noise, mute, thermal and channel-order checks into the two KAB9 boards.
8. **G8 — appliance acceptance:** Netflix → eARC → JOC → 7.1.4 runs continuously for two hours with no audio underrun, no channel swap, no unsafe startup transient and acceptable A/V lip sync.

## Fail-safe behavior

- Amplifiers remain muted until clocks are stable and the 7.1.4 route has been initialized.
- A lost eARC clock, lost IEC burst stream, decoder failure or renderer failure must mute output before restart.
- Unknown IEC 61937 data types are logged and ignored by the R0 E-AC-3 path.
- Do not automatically fall back from JOC to an unlabelled 5.1 result and report success; degradation must be explicit.

## Legacy architecture policy

The N100 + STM32H753, Raspberry Pi/CM5, MCHStreamer, ADAU1466 and earlier DAC chains remain historical research. They may be used as diagnostic fallback hardware, but are not the R0 bill of materials unless a current R0 gate proves the i.MX93 architecture cannot meet its requirement.

See `docs/imx93-r0/LEGACY_MIGRATION.md`.
