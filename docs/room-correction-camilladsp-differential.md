# RoomEQ -> CamillaDSP PCM execution differential

This Phase 10 lane validates a bounded set of RoomEQ export semantics by executing generated configurations with a real CamillaDSP binary built from Aurora's exact pinned CamillaDSP source revision.

## Exact references

- RoomEQ / `pierreaubert/autoeq`: `579dd7486024fc18ff219e31eb7337362814f602`.
- CamillaDSP / `HEnquist/camilladsp`: `05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74`, expected version `4.1.3`.

Neither project is linked into Aurora core. Both remain external validation/execution references.

## Required PCM contracts

Aurora invokes RoomEQ's upstream `scripts/run_camilladsp_backend_contracts.py` with the exact locally built CamillaDSP binary. The upstream runner requires, and refuses to silently skip, contracts covering:

- fractional group delay against the exported response;
- polarity and delay preservation;
- convolution/FIR sidecar execution;
- Linkwitz-Riley crossover gain;
- peaking-EQ gain;
- routed channel-matrix preservation;
- multi-sub coherent peak behavior at the tested sample rates.

The runner records the CamillaDSP version, command, return code, required tests, missing tests, and total tests passed in a machine-readable artifact. Aurora independently checks that artifact against its own contract.

## Fail-closed unsupported semantics

Aurora separately runs RoomEQ's `camilladsp_rejects_unsupported_plugins_instead_of_dropping_them` contract. The purpose is architectural: if a graph cannot be represented faithfully, export must fail rather than silently remove semantics.

This lane therefore does not claim arbitrary graph compatibility. For example, upstream RoomEQ explicitly has unsupported CamillaDSP export cases such as some FIR crossover/plugin combinations. Those remain unsupported until a separately proven representation exists.

## Reproducibility controls

CI fetches both repositories at exact commits, builds the CamillaDSP binary with its locked dependency graph, verifies the exact Git heads, runs the real external PCM contracts, and requires RoomEQ's `Cargo.lock` to remain unchanged during the backend run. Aurora then mutates the backend evidence to report one required contract as missing and requires its analyzer to fail closed.

## Truth boundary

A green result proves only the selected deterministic software PCM/export execution semantics at the exact two pinned source revisions. It is stronger than syntax-only `camilladsp --check`, but it is still software evidence. It does not prove physical DAC output, amplifier/speaker headroom, room correction measured by a microphone, listening preference, measured end-to-end latency, protected-service compatibility, or Dolby/DTS/HDMI certification.
