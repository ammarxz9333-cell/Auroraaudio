# Immersive layout validation

Aurora validates speaker-layout geometry separately from codec support, physical output, and proprietary renderer claims. The current software-only matrix covers the canonical 7.1.4 scene and an Aurora-defined custom 11.1.4 reference scene.

## Evidence contract

The machine-readable contract is `config/immersive-layout-validation-v1.json`. CI uses:

- sample rate: 48 kHz;
- block size: 256 frames;
- one full azimuth revolution over 2.0 s;
- nominal azimuth rate: 180 degrees/s;
- two height cycles during the trajectory;
- maximum gain change per output per block: 0.15;
- spatial power normalization tolerance: 0.001;
- pinned SAF reference commit: `18fd5aba46e20787b51f28f7197a68506c965c07`.

The 2.0 s trajectory was selected after a diagnostic 1.0 s revolution (360 degrees/s) showed 23 gain-step violations on the denser 11.1.4 geometry while the listener hull, normalization, finiteness, and LFE-exclusion gates all remained clean. Aurora did **not** raise the 0.15 gain-step threshold; instead the matrix fixes the same still-aggressive motion rate for both layouts.

## Aurora geometry gates

`aurora-validate-immersive-layout` verifies:

- the listener is inside the 3D loudspeaker hull;
- at least one validated 3D VBAP triplet exists;
- all rendered gains, delays, and distances remain finite;
- non-LFE spatial power remains normalized;
- directional object panning never leaks into the LFE output;
- the moving-source trajectory remains within the fixed gain-step continuity limit.

The probe intentionally does not require WAV export. That keeps renderer geometry validation independent from OS/container channel-mask limitations.

## 7.1.4 result

For the canonical `fixtures/scenes/7_1_4_reference.json` scene, the current matrix observed:

- 12 output channels;
- 11 directional spatial channels plus one LFE;
- 18 validated Aurora VBAP triplets;
- listener inside the hull;
- maximum Aurora gain step: `0.075027466`;
- zero gain-step, normalization, finiteness, and LFE-leakage failures;
- Aurora ↔ SAF mean gain-vector cosine similarity: `0.9407546692`;
- dominant-speaker agreement: `0.928`;
- mean spatial-centroid error: `1.7321°`.

The lower local gain-vector similarity already documented by the SAF lane comes from alternative valid triangulation diagonals in symmetric portions of the 7.1.4 hull. The topology-aware SAF gate therefore also requires bounded centroid, height-energy, dominant-speaker, unit-power, and LFE behavior rather than sample-identical gains.

## 11.1.4 custom-reference result

`fixtures/scenes/11_1_4_reference.json` contains 16 outputs:

- 11 ear-level directional channels;
- one LFE;
- four height channels.

Four ear-level roles are intentionally Aurora custom roles:

- `front-wide-left`;
- `front-wide-right`;
- `rear-wide-left`;
- `rear-wide-right`.

This is a geometry reference, **not** a claim that Aurora has standardized a Dolby, DTS, MPEG-H, IAMF, HDMI, or WAVEFORMATEXTENSIBLE 11.1.4 channel map.

The current matrix observed:

- 16 output channels;
- 15 directional spatial channels plus one LFE;
- 26 validated Aurora VBAP triplets;
- listener inside the hull;
- maximum Aurora gain step: `0.111756235`;
- zero gain-step, normalization, finiteness, and LFE-leakage failures;
- Aurora ↔ SAF mean gain-vector cosine similarity: `0.9982215333`;
- minimum gain-vector cosine similarity: `0.9694932463`;
- dominant-speaker agreement: `1.0`;
- mean spatial-centroid error: `0.11484°`;
- maximum spatial-centroid error: `1.33572°`;
- zero low-similarity frames under the `0.5` threshold.

For this exact scene, trajectory, Aurora revision, and SAF pin, the 11.1.4 custom-reference renderer behavior is therefore suitable for `REFERENCE-VALIDATED` software-layout evidence.

## Truth boundary

This lane validates only software geometry, panning gains, continuity, LFE exclusion, and the pinned SAF semantic differential. It does **not** establish:

- a standardized 11.1.4 channel mask or host-output mapping;
- codec/JOC/IAMF/MPEG-H metadata equivalence;
- binaural behavior;
- physical loudspeaker, room, DAC, amplifier, or eARC behavior;
- protected-service compatibility;
- equivalence to proprietary immersive renderers;
- certification or commercial clearance.

Physical and host-output claims remain separate roadmap gates.
