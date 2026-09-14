# Spatial Audio Framework (SAF) VBAP reference

Aurora evaluates the Spatial Audio Framework (SAF) as an **external validation oracle** for open spatial-audio DSP. SAF is not an Aurora runtime dependency in this phase.

## Pin and license boundary

- Upstream: `https://github.com/leomccormack/Spatial_Audio_Framework`
- Pinned commit: `18fd5aba46e20787b51f28f7197a68506c965c07`
- Selected core license: ISC
- Explicitly excluded optional modules: `saf_tracker` and `saf_hades` (GPLv2)

Aurora CI configures those GPL modules off. The initial lane also disables the optional SOFA-reader module because HRTF/binaural validation belongs to a later roadmap phase.

The pin is immutable validation input. CI must not follow `master`, `develop`, `latest`, or an unreviewed release implicitly.

## Why SAF is useful

SAF contains a mature 3D VBAP implementation and a broader set of Ambisonics, spherical-harmonic, HRTF/HRIR, and spatial-audio DSP primitives. Aurora uses it to compare **spatial semantics and geometry**, not to require identical implementation details.

The first differential target is Aurora's canonical `fixtures/scenes/7_1_4_reference.json`. The LFE speaker is excluded from directional panning on the Aurora side and must not be supplied as a directional loudspeaker to the SAF VBAP reference.

## Validation stages

### Stage 1: pinned upstream baseline

CI must:

1. checkout the exact SAF commit;
2. verify the source pin and the ISC/GPL module boundary;
3. build SAF with OpenBLAS/LAPACKE;
4. force tracker and HADES off;
5. run the upstream SAF test executable.

This stage proves only that the selected permissive core builds and its upstream tests pass in Aurora's CI environment.

### Stage 2: Aurora ↔ SAF 3D VBAP differential

A follow-up probe uses the same 7.1.4 geometry in both renderers and compares normalized gain vectors for deterministic source directions and trajectories. Acceptance is based on:

- finite, non-negative gains;
- normalized spatial power;
- dominant-speaker agreement at speaker anchors;
- left/right mirror symmetry;
- bounded azimuth/elevation spatial-centroid error;
- sensible height-energy transitions;
- trajectory continuity;
- strict LFE exclusion.

Different valid triangulations may produce different gain distributions between anchors. Therefore **sample-identical PCM and gain-identical vectors are not acceptance requirements**. Any numerical threshold must be evidence-driven and recorded in the generated artifact rather than chosen to hide a mismatch.

## Evidence boundary

A green SAF lane may qualify a specific Aurora renderer behavior as `REFERENCE-VALIDATED` for the tested geometry and probes. It does not prove:

- arbitrary-layout correctness outside the tested corpus;
- binaural/HRTF equivalence;
- HOA equivalence;
- physical loudspeaker or room behavior;
- 11.1.4 support unless separately exercised;
- Dolby, DTS, MPEG-H, IAMF, or other proprietary/reference-renderer equivalence;
- product certification or commercial clearance.

SAF remains an external reference unless a later, explicit architecture and license review changes its role.
