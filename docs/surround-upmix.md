# Channel surround upmix

Aurora keeps channel-based upmix evidence explicitly separate from JOC/OAMD, IAMF, MPEG-H, and authored-object rendering.

## Legacy IEC61937 integration lane

`validation/surround-upmix/aurora-surround-upmix.sh` is an explicitly channel-based experimental ingest path.

It decodes AC-3/E-AC-3 from IEC61937 with FFmpeg, preserves the decoded bed, and derives quiet decorrelated height ambience. It **does not** decode JOC/OAMD objects and must never be reported as Atmos/object recovery.

Run its software test with:

```bash
python3 validation/surround-upmix/test_surround_upmix.py
```

The test verifies bed preservation, synthetic height activity for suitable material, dialogue/LFE isolation, output before EOF, invalid-input rejection, and host-side throughput. Throughput is not a physical latency measurement.

## Phase 9 decoded-PCM upmix matrix

`validation/surround-upmix/aurora-pcm-upmix.sh` deliberately starts **after codec decode**. This isolates upmix DSP semantics from codec/container limitations and allows a real 7.1 bed to be tested without relying on an encoder to preserve an eight-channel E-AC-3 fixture.

The required matrix is defined in `config/upmix-validation-v1.json`:

- 5.1 -> 7.1.4;
- 5.1 -> Aurora's custom 11.1.4 reference order;
- 7.1 -> Aurora's custom 11.1.4 reference order.

Original bed channels are copied unchanged to declared target indices. Additional back/wide/height channels are low-level linear difference-derived ambience. Center and LFE are excluded from all synthetic-channel equations.

`validation/surround-upmix/test_pcm_upmix_matrix.py` measures and gates:

- sample-for-sample bed preservation;
- bounded synthetic-to-bed RMS energy;
- synthetic/source correlation so an added channel is not a near-clone of one source channel;
- pairwise spectral balance on deterministic multitone probes;
- center and LFE isolation;
- phase-coherent dual-mono cancellation;
- 4x input -> 4x synthetic-output linearity as a no-pumping/dynamic-gain check;
- stress peak/headroom below full scale;
- finite deterministic PCM and exact frame count.

The CI lane builds exact reviewed FFmpeg commits and runs the same matrix against each selected FFmpeg line. Codec ingress remains covered by the separate IEC61937/FFmpeg compatibility lane.

## Truth boundary

The added wide/back/height signals are **synthetic channel ambience**. A passing upmix matrix is not evidence of JOC/Atmos object recovery, original mix intent, object positions, protected-service compatibility, physical room behavior, room correction, wireless synchronization, or certification.
