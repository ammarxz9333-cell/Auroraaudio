# Room-correction reference baseline

Aurora Pre-Hardware Roadmap Phase 10 uses two exact-pinned external references. Neither project is linked into Aurora core.

## Roles

`pierreaubert/autoeq` / RoomEQ is the optimization and acoustic-system reference. Aurora pins commit `579dd7486024fc18ff219e31eb7337362814f602`, observed workspace version `0.5.73`. The upstream root package declares `GPL-3.0-or-later`, so Aurora keeps it in an external process/reference boundary.

The selected reference surface covers multichannel room correction, FIR/IIR and hybrid processing, crossover/bass-management work, multi-sub/timing/phase work, safety/quality gates, and export contracts. CI runs a deliberately focused upstream subset rather than claiming the entire upstream QA matrix as Aurora evidence.

`HEnquist/camilladsp` is the DSP-execution reference. Aurora pins commit `05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74`, version `4.1.3`. Upstream declares `GPL-3.0-only OR MPL-2.0`. Aurora continues to use it only behind an external-process boundary.

## Current executable baseline gate

`Room Correction Reference CI` does all of the following from clean checkouts:

1. fetches the exact RoomEQ commit and rejects any identity mismatch;
2. runs the selected `roomeq-engine` and `roomeq-qa` contracts;
3. runs the exact pinned RoomEQ DSP-realization regression `dsp_realization::tests::mixed_fir_iir_replay_matches_independent_complex_branch_sum` and requires the Rust test summary to report exactly `1 passed; 0 failed`, so a zero-test filter cannot pass silently;
4. fetches the exact CamillaDSP commit and rejects any identity mismatch;
5. generates a dependency lock for the pinned CamillaDSP source revision, records its SHA-256, then runs CamillaDSP library tests with `--locked` and without default optional features;
6. validates source version, source license, required source surfaces and generated dependency-lock evidence against `config/room-correction-reference-v1.json`;
7. emits machine-readable Aurora evidence;
8. mutates the expected RoomEQ pin and requires the analyzer to fail closed.

This establishes the reproducible external-reference baseline. It does not by itself prove an Aurora room-correction result, physical speaker/sub alignment, acoustic improvement or measured latency.

## Additional Phase 10 evidence

PR #174 adds the deterministic synthetic multichannel RoomEQ lane. It exercises 7.1.4 optimization, all four PR-eligible LFE/sub topologies, multi-seat phase guards, final-chain headroom/peak constraints, Stage 3 policy plumbing, and corrupted-log/pin negative controls. The lane remains synthetic software-reference evidence, not microphone evidence.

PR #175 adds the separate RoomEQ -> CamillaDSP export/execution differential. It builds exact-pinned CamillaDSP, validates generated configuration, executes the required real-PCM contracts, proves unsupported graph semantics fail closed, and runs Aurora's real 12-channel 7.1.4 channel-order sentinel through CamillaDSP. The declared 7.1.4 logical-to-WAVE mapping is `[0,1,2,3,6,7,4,5,8,9,10,11]`, with front center preserved at index 2.

Together these lanes close Phase 10 only for the explicitly declared software/reference scope after their final-head CI is green. They do not establish arbitrary DSP-graph support, physical DAC/speaker routing, microphone-measured room correction, protected-service interoperability or certification.

## Truth boundary

A green Phase 10 software/reference set means Aurora can reproduce the exact external source identities and licensing boundaries, exercise the declared non-vacuous upstream baseline tests, execute the selected deterministic synthetic optimizer semantics, and verify the declared RoomEQ-to-CamillaDSP PCM contract subset plus role-aware 7.1.4 channel mapping. Physical/acoustic acceptance remains separate evidence under the hardware critical path.
