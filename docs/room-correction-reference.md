# Room-correction reference baseline

Aurora Pre-Hardware Roadmap Phase 10 starts with two exact-pinned external references. Neither project is linked into Aurora core.

## Roles

`pierreaubert/autoeq` / RoomEQ is the optimization and acoustic-system reference. Aurora pins commit `579dd7486024fc18ff219e31eb7337362814f602`, observed workspace version `0.5.73`. The upstream root package declares `GPL-3.0-or-later`, so Aurora keeps it in an external process/reference boundary.

The selected reference surface covers multichannel room correction, FIR/IIR and hybrid processing, crossover/bass-management work, multi-sub/timing/phase work, safety/quality gates, and export contracts. CI runs a deliberately focused upstream subset rather than claiming the entire upstream QA matrix as Aurora evidence.

`HEnquist/camilladsp` is the DSP-execution reference. Aurora pins commit `05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74`, version `4.1.3`. Upstream declares `GPL-3.0-only OR MPL-2.0`. Aurora continues to use it only behind an external-process boundary.

## Current executable gate

`Room Correction Reference CI` does all of the following from clean checkouts:

1. fetches the exact RoomEQ commit and rejects any identity mismatch;
2. runs selected upstream `roomeq-engine`, `roomeq-workflow`, and `roomeq-qa` contracts;
3. fetches the exact CamillaDSP commit and rejects any identity mismatch;
4. runs CamillaDSP library tests without default optional features;
5. validates source version, source license, and required source surfaces against `config/room-correction-reference-v1.json`;
6. emits machine-readable Aurora evidence;
7. mutates the expected RoomEQ pin and requires the analyzer to fail closed.

This establishes a reproducible external-reference baseline. It does **not** yet prove that an Aurora measurement is optimized correctly, that a RoomEQ export is reproduced exactly by CamillaDSP, or that any physical room/speaker response improves.

## Next Phase 10 evidence

The next bounded lane should use deterministic synthetic multichannel responses to exercise correction behavior without pretending synthetic data is a microphone measurement. It must cover at least dialogue/main-channel preservation, LFE/bass-management routing, crossover behavior, correction headroom, clipping prevention, and timing/phase semantics.

After that, a separate export/execution differential should generate a representable RoomEQ DSP graph, export it for CamillaDSP, process the same deterministic multichannel PCM through the reference graph and executor, and compare transfer-function/output semantics within declared tolerances. Unsupported graph features must fail closed rather than being silently simplified.

Physical microphone sweeps, room impulse responses, speaker/sub timing, listening-area optimization, acoustic before/after metrics, and physical latency remain separate hardware/acoustic acceptance evidence.

## Truth boundary

A green Phase 10 baseline means only that Aurora can reproduce the exact external source versions, their declared licensing boundary, required source/API surfaces, selected upstream tests, and fail-closed pinning. Room correction itself remains `REFERENCE-VALIDATION-IN-PROGRESS` until the deterministic optimizer and export/execution differential lanes are green.
