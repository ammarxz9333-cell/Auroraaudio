# GenAVB one-listener evidence runner

Status: hardware-ready orchestration helper. It does not capture hardware and does not convert CI or fixture data into physical proof.

## Purpose

Aurora's one-listener physical gate intentionally keeps the three evidence sources independent:

1. host / AVDECC send evidence;
2. NXP gPTP before/after snapshots;
3. ESP listener before/after snapshots.

`validation/physical/aurora_genavb_one_listener_run.py` reduces operator error after those captures exist. It builds the two intermediate evidence objects and invokes the final fail-closed correlator in one command.

## Inputs

All five input files must describe the same physical run and the same operator/controller-supplied epoch:

- `host.json` — emitted by `physical_single_listener_probe`;
- `nxp-before.json` — NXP exact-public-API gPTP snapshot before the send interval;
- `nxp-after.json` — NXP snapshot after the send interval;
- `esp-before.json` — ESP listener snapshot before the send interval;
- `esp-after.json` — ESP listener snapshot after the send interval.

The exact capture procedure remains documented in `docs/genavb-single-listener-physical-probe.md`.

## Physical run

Physical mode must be selected explicitly; omitting both mode flags is an argument error.

```bash
python validation/physical/aurora_genavb_one_listener_run.py bundle \
  --host host.json \
  --nxp-before nxp-before.json \
  --nxp-after nxp-after.json \
  --esp-before esp-before.json \
  --esp-after esp-after.json \
  --epoch-id 20260917T090000Z-run01 \
  --output-dir evidence/run01 \
  --physical-run
```

Use `--physical-run` only when every input is an independently captured real-hardware artifact from the same epoch. The runner validates the evidence contracts but cannot authenticate provenance.

For validator fixtures, use `--fixture-mode` instead. The two mode flags are mutually exclusive and one is required.

The runner writes:

- `nxp-gptp-evidence.json`;
- `esp-listener-evidence.json`;
- `one-listener-correlation.json`;
- `run-manifest.json`.

The manifest schema is `aurora.genavb.one-listener-run.v1` and records whether the invocation was `physical` or `fixture` mode.

## Verdicts

- `FAIL` — at least one required evidence contract, identity, clock, receive-counter or timing condition failed.
- `VALIDATOR-PASS` — only possible when `--fixture-mode` is explicitly supplied; this is software/validator evidence and `physical_complete` remains false.
- `PHYSICAL-PASS` — only possible when `--physical-run` is explicitly supplied and the existing final correlator accepts the host/NXP/ESP evidence.

The runner does not relax any existing gate. It delegates NXP snapshot validation to `aurora_genavb_nxp_gptp_evidence.py`, ESP snapshot validation to `aurora_esp_avb_listener_evidence.py`, and the final same-epoch correlation to `aurora_genavb_single_listener_evidence.py`.

## Self-test

```bash
python validation/physical/aurora_genavb_one_listener_run.py self-test
```

The self-test contains one positive fixture that must stop at `VALIDATOR-PASS`, one negative RX-counter case that must fail, and an explicit physical-mode argument parsing check. CI compiles and runs this helper on Linux and Windows.

## Truth boundary

A green CI run proves only that the orchestration logic compiles and preserves the existing fail-closed evidence semantics. It does not prove AVDECC/ACMP interoperability, AAF receive, gPTP lock, synchronization, latency, RF behavior or acoustics on physical hardware. Even `--physical-run` is an operator declaration, not cryptographic provenance attestation.
