# Decoder-to-DSP channel order

The external Omniphony 7.1.4 layout, FFmpeg 7.1.4 output and Aurora USB v1
use back-surround channels before side-surround channels:

`FL FR C LFE BL BR SL SR TFL TFR TBL TBR`

`aurora_core::StandardLayout::SevenOneFour.canonical_roles()` and the shared
`SpeakerPostProcessor` calibration use side-surround channels before back:

`FL FR C LFE SL SR BL BR TFL TFR TBL TBR`

Previously the S6 postprocessor passed decoder output directly into the shared
DSP. Flat settings could hide the mismatch, but role-specific side/back trims,
polarity, equalization and delays were applied to the wrong pair of speakers.

`process_wire_block` now permutes indices 4/6 and 5/7 before shared DSP processing
and restores wire order afterwards. ASRC and the external pipes retain wire
order. USB protocol, layout hash, external decoder arguments, shared DSP order
and existing calibration files are unchanged. All twelve channels remain
independent; this change does not add upmixing or invent object metadata.

The conversion operates in place with two swaps per frame on each side of the
DSP call. It allocates no storage and adds no queued audio or delay samples.
Incomplete frames are silenced and rejected before DSP state is changed.

## Regression tests

The S6 binary tests give each wire channel a distinct impulse and each canonical
speaker role a distinct trim, delay and polarity. Output is compared sample by
sample against a separately prepared canonical DSP reference with explicit role
mapping. An allocation audit covers the complete wire-to-DSP-to-wire operation.
A separate malformed-block test checks silence and rejection.

Run the existing S6 CI command:

```sh
cargo test --locked -p aurora-cli --no-default-features --features realtime --bin aurora-s6-postprocess
```

Local validation on 2026-09-06: channel contracts were checked directly in the
external layout, USB header, canonical role table and calibration implementation.
Cargo/rustc are unavailable in this execution environment; the new Rust tests,
format, clippy and benchmark gates have NOT been executed successfully here.
No physical or perceptual result is claimed. This fixes a software routing
boundary; it does not establish JOC decoding or live-service acceptance.
