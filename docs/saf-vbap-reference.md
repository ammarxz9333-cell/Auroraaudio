# Spatial Audio Framework (SAF) VBAP reference

Aurora evaluates the Spatial Audio Framework (SAF) as an **external validation oracle** for open spatial-audio DSP. SAF is not an Aurora runtime dependency in this phase.

## Pin and license boundary

- Upstream: `https://github.com/leomccormack/Spatial_Audio_Framework`
- Tested upstream version: `1.3.5`
- Pinned commit: `18fd5aba46e20787b51f28f7197a68506c965c07`
- Selected validation subset: ISC-licensed SAF core/VBAP functionality
- Excluded functionality: `saf_tracker` and `saf_hades` (GPLv2)

A license-sensitive upstream build-graph issue is explicitly handled by Aurora. At the pinned commit, the top-level SAF CMake source list includes the GPLv2 `saf_tracker` translation units in the unconditional core target even when `SAF_ENABLE_TRACKER_MODULE=OFF`, and the unit-test target similarly lists GPLv2 tracker/HADES test translation units. Aurora therefore applies `validation/open-immersive/saf-permissive-core.patch` **only inside the external validation checkout** to remove those disabled GPL translation units from the build graph. CI exports `compile_commands.json` and fails if a tracker/HADES module or test translation unit is compiled.

This is not an upstream source fork and no patched SAF binary is shipped as part of Aurora. It is a reproducible CI boundary used to keep this validation lane separated from GPL code while evaluating the ISC VBAP implementation.

The optional SOFA-reader module is also disabled because HRTF/binaural validation belongs to a later roadmap phase. The pin is immutable validation input; CI must not follow `master`, `develop`, `latest`, or an unreviewed release implicitly.

## Why SAF is useful

SAF contains a mature 3D VBAP implementation and a broader set of Ambisonics, spherical-harmonic, HRTF/HRIR, and spatial-audio DSP primitives. Aurora uses it to compare **spatial semantics and geometry**, not to require identical implementation details.

The first differential target is Aurora's canonical `fixtures/scenes/7_1_4_reference.json`. The LFE speaker is excluded from directional panning on the Aurora side and is not supplied as a directional loudspeaker to the SAF VBAP reference.

## Validation stages

### Stage 1: pinned permissive baseline

CI:

1. checks out the exact SAF commit;
2. identifies the upstream ISC/GPL boundary;
3. removes disabled GPL tracker/HADES module and test translation units from the validation build graph;
4. configures SAF with OpenBLAS/LAPACKE and tracker/HADES/SOFA-reader disabled;
5. verifies the exported compile graph contains none of those GPL translation units;
6. builds the remaining SAF validation subset;
7. runs the remaining upstream SAF test executable.

This stage proves only that the pinned, selected validation subset builds and its applicable upstream tests pass in Aurora's CI environment.

### Stage 2: Aurora ↔ SAF 3D VBAP differential

The differential uses the same 7.1.4 loudspeaker geometry and the same deterministic moving-source trajectory in both renderers. Aurora's fixture coordinate system is converted explicitly to SAF azimuth/elevation, LFE is removed from the directional set, and both gain vectors are evaluated independently.

The initial evidence exposed an important property of the symmetric 7.1.4 hull: Aurora and SAF can choose different valid diagonals across a symmetric rear loudspeaker quad. Around that local topology boundary, direct gain-vector cosine similarity can become low even while the rendered spatial centroids remain close. Therefore a single-frame minimum cosine is not a valid equivalence gate for this geometry.

The final acceptance model is topology-aware. It requires:

- finite, non-negative gains in both renderers;
- strict LFE exclusion;
- squared-gain power error no greater than `0.001`;
- mean normalized gain-vector cosine similarity at least `0.90`;
- fewer than or equal to `10%` of frames below cosine `0.50`;
- dominant-speaker agreement at least `90%`;
- mean spatial-centroid error no greater than `5°` and maximum no greater than `30°`;
- mean height-energy absolute delta no greater than `0.05` and maximum no greater than `0.20`;
- explicit evidence for the worst gain-vector frames, including source direction, dominant speakers, centroid error, height-energy delta, and sparse gains.

These gates deliberately permit a **bounded localized alternative-triangulation region** but do not permit broad renderer disagreement. Sample-identical PCM and gain-identical vectors are not acceptance requirements.

The pre-adjustment evidence that motivated this rule showed, over 188 trajectory frames: mean cosine `0.9409`, dominant-speaker agreement `0.9255`, mean centroid error `1.73°`, maximum centroid error `23.03°`, mean height-energy delta `0.00883`, no non-finite/negative gains, no LFE leakage, and only 10/188 frames below cosine `0.50`. The low-similarity frames clustered around the symmetric rear transition rather than across the trajectory. These observations are diagnostic context; the CI still enforces the round, predeclared thresholds above on every run.

## Evidence boundary

A green SAF lane may qualify the tested Aurora **7.1.4 software renderer behavior** as `REFERENCE-VALIDATED` only for this pinned SAF reference, geometry, trajectory, and metric set. It does not prove:

- arbitrary-layout correctness outside the tested corpus;
- binaural/HRTF equivalence;
- HOA equivalence;
- physical loudspeaker or room behavior;
- 11.1.4 support unless separately exercised;
- Dolby, DTS, MPEG-H, IAMF, or other proprietary/reference-renderer equivalence;
- product certification or commercial clearance.

SAF remains an external reference unless a later, explicit architecture and license review changes its role.
