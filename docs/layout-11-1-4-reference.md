# Aurora 11.1.4 soundbar-oriented research layout

Aurora validates a custom **11.1.4** software geometry as a research target for soundbar-class rendering. This fixture is intentionally treated as a custom Aurora layout rather than as a claim that the channel-role mapping is a Dolby, DTS, Samsung, ITU, or other proprietary standard.

## Channel model

The fixture contains 16 enabled outputs:

- 11 horizontal channels;
- 1 LFE channel;
- 4 top channels.

The horizontal set is composed of the canonical Aurora front/center, surround, and surround-back roles plus four custom roles:

- `front-wide-left`;
- `front-wide-right`;
- `rear-side-left`;
- `rear-side-right`.

The purpose of the custom roles is to exercise a denser soundbar-oriented horizontal hull without prematurely extending Aurora's canonical `StandardLayout` contract. A later hardware/acoustic phase may revise the exact physical mapping while preserving the validated renderer invariants.

## Validation stages

### Aurora-only geometry gate

`aurora-validate-immersive-layout` must prove, for the exact fixture:

- 16 enabled outputs;
- 15 spatial outputs after excluding LFE;
- exactly 1 LFE output;
- exactly 4 top outputs;
- exactly 4 custom-role outputs;
- listener inside the validated 3D loudspeaker hull;
- finite renderer output;
- unit spatial power within the configured tolerance;
- bounded gain steps on the deterministic moving-source probe;
- zero object-panning energy on LFE.

### Aurora ↔ SAF differential

The exact same loudspeaker geometry and source trajectory are supplied to the pinned Spatial Audio Framework reference lane. Acceptance is semantic and geometric rather than sample-identical:

- finite and non-negative gains;
- normalized spatial power;
- bounded low-similarity frame fraction;
- aggregate gain-vector similarity;
- dominant-speaker agreement;
- bounded spatial-centroid error;
- bounded height-energy difference;
- strict LFE exclusion.

Alternative valid 3D triangulations are allowed within the predeclared outlier budget. Thresholds are committed in `config/layout-11-1-4-reference-v1.json` before observing the CI result; they must not be silently relaxed to hide a renderer mismatch.

## Evidence status

Before a green reference CI run, this layout remains `SIMULATED-UNTIL-CI-PASSES`. A green run may record `REFERENCE-VALIDATED` only for the exact fixture, Aurora revision, SAF pin, and metric set captured in the evidence artifact.

## Truth boundary

This validation does **not** prove:

- reflected up-firing acoustics in a real room;
- equivalence to any commercial 11.1.4 soundbar channel mapping;
- wireless rear-speaker synchronization;
- room correction or bass management;
- eARC or codec interoperability;
- Dolby/DTS/proprietary renderer equivalence;
- certification or commercial clearance.

Those remain separate physical, network, codec, and legal gates in the pre-hardware roadmap.
