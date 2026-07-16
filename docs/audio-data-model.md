# Audio Data Model

The first checkpoint defines the canonical typed structures in `aurora-core`:

- `AudioFormat`
- `SampleType`
- `Vector3`
- `Speaker`
- `Listener`
- `AudioObject`
- `AudioBlock`
- `Scene`
- `ChannelRole`
- `StandardLayout`

All positions are expressed in meters. Audio samples are represented as planar `f32` blocks.

## Channel Order

Aurora uses explicit `ChannelRole` values and never infers channel semantics from speaker coordinates.

Canonical standard output order:

- Stereo: FL, FR
- 5.1: FL, FR, FC, LFE, SL, SR
- 7.1: FL, FR, FC, LFE, SL, SR, SBL, SBR
- 5.1.2: FL, FR, FC, LFE, SL, SR, TFL, TFR
