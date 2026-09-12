# Aurora immersive/JOC validation

This directory contains software evidence lanes.  It does **not** establish Dolby certification, protected-streaming compatibility, physical eARC/TDM hardware behavior, or acoustic equivalence to a commercial sound system.

## Evidence lanes

| Lane | Establishes | Does not establish |
| --- | --- | --- |
| `test-joc-stack.sh` | pinned Harletty/Omniphony software path and 7.1.4-shaped output | independent object-position truth |
| `test-joc-realtime-soak.sh` | repeated IEC61937/JOC metadata survival and paced 7.1.4 output | moving-object diversity; physical realtime hardware |
| `test-openjoc-reference.sh` | independent OpenJOC JOC/admission/timing census and experimental 7.1.4 render | authored object-position correctness |
| `test-joc-differential.sh` | independent implementation agreement on active-channel identity plus non-gating correlations | all 12 channels active; moving objects |
| `test-joc-temporal-evidence.sh` | fail-closed separation of timed OAMD/object-state diversity and windowed rendered-energy diversity | proof that rendered speaker energy is an independent oracle for authored trajectories |

## Temporal JOC evidence

Usage:

```bash
bash validation/immersive/test-joc-temporal-evidence.sh \
  INPUT.eac3 \
  EXPECTED_SHA256 \
  "provenance text" \
  OUTPUT_DIR
```

The input SHA-256 and provenance are mandatory.  The harness first runs the pinned OpenJOC reference gate, retains access-unit timestamps, renders experimental 7.1.4, normalizes the render to 12-channel f32, and then calls `joc_temporal_evidence.py`.

A temporal pass requires all of the following:

1. positive JOC identification, complete E-AC-3 access units, decoder admission, continuous frame and metadata timing, deployed-compatibility pass, and complete diagnostics;
2. OpenJOC scene/OAMD evidence of dynamic metadata with at least one object whose observed `position_min` and `position_max` differ;
3. more than one meaningful time-windowed rendered-energy profile, measured as changed active-lane sets or a configured normalized-energy L1 distance.

The metadata and render tests are intentionally independent.  `render-joc` is an experimental self-consistency renderer; a channel-energy change is **not** evidence that a particular channel independently verifies an authored object position.

Exit status `3` means the carrier passed the codec/JOC contract but did not establish enough temporal diversity.  This is an expected fail-closed result for a static or temporally weak carrier, not a codec failure.

`python3 validation/immersive/joc_temporal_evidence.py self-test` exercises only deterministic metric/report logic using synthetic data.  Synthetic data is never counted as codec/JOC evidence.

## Current public fixture limitation

The pinned public Harletty `joc_atmos_1s.eac3` fixture remains useful for the existing JOC/differential/realtime lanes, but its public object metadata is not a moving-object corpus.  CI therefore requires it to pass the codec/JOC gate and fail the temporal harness specifically with `insufficient_temporal_diversity`.  Aurora must not promote that fixture as moving-object proof.

A carrier that passes the temporal harness may be supplied externally when its provenance and checksum are known.  Copyrighted/private Atmos media should not be committed merely to make CI pass.
