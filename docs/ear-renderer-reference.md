# EBU EAR renderer reference lane

Aurora uses the EBU ADM Renderer (EAR) only as an external validation oracle in this lane. No EAR code is vendored or linked into Aurora core, and this change does not add an Aurora ADM runtime parser or renderer backend.

## Exact reference

- Upstream: `ebu/ebu_adm_renderer`
- Version reported by the pinned source: `2.1.0`
- Commit: `5bb17b4278e8e67f24f90efb7a81a8aea7aed4f3`
- License: BSD-3-Clause-Clear. The upstream license explicitly grants no patent rights.
- Aurora integration class: `external-process-validation-reference`

The CI lane checks the exact git commit and the git blob identities of the three upstream fixture inputs before doing any rendering.

## Bounded fixture

The lane intentionally uses EAR's own public integration fixture:

- `ear/test/data/test.wav`
- `ear/test/data/test.yaml`
- `ear/test/data/test_bwf.wav`

The YAML scene contains two object-based items plus DirectSpeakers content and LFE. Aurora first asks the pinned `ear-utils make_test_bwf` to regenerate the BWF from the pinned WAV and YAML and requires the result to be byte-identical to the pinned upstream `test_bwf.wav`. This qualifies the exact fixture and generator path without copying the binary fixture into Aurora.

## Renderer evidence

The generated BWF is rendered by the exact pinned `ear-render` into two ITU-R BS.2051 target systems:

- `4+5+0`: 10 output channels. Aurora's independent evidence analyzer reproduces the mapping asserted by EAR's upstream `test_integrate.py::test_render`, including the 1/8-second object jump, DirectSpeakers duplication, height-channel routing, and LFE routing. The normalized PCM must stay within the configured absolute-error bound.
- `4+7+0`: 12 output channels. The lane requires the same frame count, finite/non-silent PCM, the expected channel count, and a bounded minimum number of active channels. This is a layout/render smoke oracle, not an Aurora-vs-EAR semantic-equivalence claim yet.

The lane also feeds the plain pinned WAV (without ADM metadata) to `ear-render` and requires a non-zero exit code. This checks that the reference fails closed for that negative input rather than silently treating it as an ADM scene.

Only JSON/probe/stderr evidence is uploaded. The generated BWF and rendered audio are not retained as Aurora workflow artifacts.

## What a green lane proves

A green lane qualifies the exact pinned EAR source as an external ADM rendering reference for this one public fixture. It proves deterministic fixture generation, the bounded `4+5+0` semantic mapping reproduced independently by Aurora's analyzer, finite frame-stable rendering of the same fixture to `4+7+0`, and the negative no-ADM behavior.

It does **not** prove:

- Aurora ADM/BW64 ingestion or runtime rendering;
- arbitrary ADM scene equivalence;
- exhaustive ITU-R BS.2076 or BS.2127 conformance;
- every BS.2051 layout;
- sample-identical output between independent renderers;
- 7.1.4/11.1.4 Aurora differential equivalence;
- physical output, protected-service interoperability, patent clearance, or certification.

The next renderer-validation step is a separate algorithmic lane using the Spatial Audio Framework (SAF) for VBAP/HOA/DSP comparisons. EAR remains the ADM/BS.2127-oriented reference; SAF must not be treated as a substitute for that standards role.
