# Deterministic synthetic RoomEQ lane

This Phase 10 lane validates a bounded software-only RoomEQ correction path at Aurora's exact pinned RoomEQ commit. It intentionally uses RoomEQ's deterministic in-memory synthetic fixtures rather than microphone captures.

## Selected multichannel case

The CI runs the RoomEQ synthetic QA binary with the bounded PR matrix, `easy` difficulty, `Hybrid` processing, and the `7.1.4` layout. At the pinned upstream revision, that PR subset exercises four LFE/sub topologies for the selected layout:

- `single_sub`;
- `mso_2sub`;
- `cardioid`;
- `dba`.

The upstream multichannel guard checks the logical-channel count against the selected layout, validates expected physical-sub counts for MSO topologies, performs the optimization, and rejects meaningful score regressions unless the production-style safety path explicitly reverts the candidate. Aurora's evidence analyzer additionally requires the selected multi-channel summary to report all four cases passing and the overall synthetic outcome to contain zero failed outcomes.

A safe revert is recorded separately from a normal pass. It is a safety success, not acoustic-improvement evidence.

## Phase, timing, and chain-safety guards

A separate multi-seat guard run exercises missing-phase rejection, strategy metrics, and bounded polarity/all-pass controls. Aurora also runs RoomEQ's `roomeq-quality` chain-constraint unit tests and its Stage 3 synthetic policy demo. These cover software rules for peak gain, digital headroom, latency, export-rate constraints, bounded inversion, trusted-vs-untrusted timing, and final candidate-vs-identity validation.

These checks validate policy behavior on deterministic synthetic records. They do not measure amplifier headroom, loudspeaker excursion, physical subwoofer phase, room decay, or end-to-end latency.

## Fail-closed evidence

`validation/room-correction/room_correction_synthetic_evidence.py` requires explicit RoomEQ summary markers and exact selected multi-channel case counts. Missing markers, nonzero failures, count drift, or malformed logs fail the gate. CI also corrupts the multi-channel pass count and requires the analyzer to reject the modified evidence.

## Next lane

After this synthetic lane is green, Phase 10 should add the separate RoomEQ-to-CamillaDSP export/execution differential. That lane must compare representable DSP semantics/output for identical deterministic multichannel input, declare tolerances, and fail closed on unsupported graph features rather than silently simplifying them.

## Truth boundary

A green result means only that the exact pinned RoomEQ software passes the selected deterministic multichannel, phase-control, and chain-policy checks and that Aurora can verify those results fail-closed. It is not microphone evidence, a listening test, measured acoustic improvement, measured physical latency/headroom, or proof that CamillaDSP executes the exported graph equivalently.
