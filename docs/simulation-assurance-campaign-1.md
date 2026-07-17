# Simulation Assurance Campaign 1

Simulation Assurance Campaign 1 is a test-only deterministic property and
stress harness. It calls the accepted `aurora-realtime-audio-sim` APIs and the
existing Phase 3A/3B VBAP renderer. It is not another simulator and adds no
product runtime path.

## Bounded Model

The harness generates one scenario at a time and retains at most 32 failure
records. Passing scenarios add only to fixed-domain coverage sets and one
rolling FNV-1a checksum. Scenario count is capped at 1,000,000, repeat count at
10, shard count at 64, and accelerated soak duration at 720 hours.

Every generated scenario has:

- a deterministic seed and stable `sac1-...` identifier;
- a bounded configuration and `truth_source=deterministic_simulation`;
- a single-scenario replay command;
- a deterministic outcome or failure category;
- a bounded event window retained only on failure.

Failure JSON is written as one bounded sibling of the selected report. Those
generated files remain under ignored `output/`; confirmed defects should
receive a small reviewed regression fixture rather than a large campaign dump.

## Properties

Generated scenarios cover deterministic duplex scheduling, routing, point and
spread rendering, invalid inputs, and extreme finite renderer state. The
campaign checks finite output, bounded ring state, zero reported steady-state
memory growth, valid lifecycle transitions, explicit fault observability,
structured invalid-input errors, channel isolation, normalized renderer
energy, spread-zero compatibility, and layout-permutation equivalence.

Workspace tests remain the source for warmed-up allocation guards around
callbacks and render calls. The PR workflow runs those tests together with the
campaign. Campaign code performs no work inside a production callback.

Checksums are deterministic for a fixed supported target. Reports name the OS
and architecture because floating-point trigonometry is not claimed to be
bit-identical across targets. Host execution duration is
`host_api_observation`; it is excluded from deterministic checksums and is not
latency.

## Commands

PR smoke, 1,000 scenarios:

```powershell
cargo run --release -p aurora-simulation-assurance -- `
  --level smoke --scenarios 1000 --start-seed 0 `
  --report output\simulation-assurance\smoke-1000.json
```

Standard campaign with all accepted legacy fault fixtures:

```powershell
cargo run --release -p aurora-simulation-assurance -- `
  --level standard --scenarios 10000 --start-seed 0 `
  --report output\simulation-assurance\standard-10000.json
```

Fixed 1,000 seeds repeated three times:

```powershell
cargo run --release -p aurora-simulation-assurance -- `
  --level smoke --scenarios 1000 --start-seed 0 --repeat 3 `
  --report output\simulation-assurance\repeat-1000.json
```

Representative accelerated 24-hour profile matrix:

```powershell
cargo run --release -p aurora-simulation-assurance -- `
  --level soak --scenarios 4 --soak-hours 24 --start-seed 0 `
  --report output\simulation-assurance\soak-24h.json
```

The `reproducible_command` in a failure record uses `--replay-seed` and
`--replay-ordinal` to execute exactly one generated scenario.

## Workflows

- `simulation-assurance-pr.yml`: 500-scenario PR smoke plus workspace tests;
- `simulation-assurance-nightly.yml`: four deterministic shards totaling
  10,000 scenarios;
- `simulation-assurance-deep.yml`: eight manually dispatched shards,
  configurable to 100,000 scenarios by default;
- `simulation-assurance-soak.yml`: manually dispatched 24-hour, 7-day, or
  30-day accelerated profile matrix.

Workflow reports are bounded artifacts and are not committed.

## Evidence Boundary

Campaign results use only `unit_test`, `deterministic_simulation`, or
`host_api_observation`. They do not validate physical clocks, endpoints,
latency, routing, levels, stability, or listening behavior. Phase 2 and the
physical gates of Phase 3A/3B remain open and unchanged.
