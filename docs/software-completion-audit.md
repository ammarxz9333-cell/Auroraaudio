# Aurora software-completion audit

This branch closes software-only runtime gaps without selecting hardware or claiming physical evidence.

## Acceptance rule

A runtime crate must not ship an implementation marker (`todo!`, `unimplemented!`, or `placeholder`) on a production source path. Optional external/reference capabilities must either be functional for their declared scope or be represented as explicitly unsupported/deferred capability state rather than a fake runtime implementation.

## In scope

- P0 adaptive clock correction and hard-fault behavior.
- Bounded device reconnect/recovery.
- Panic-isolated immersive decoder/runtime recovery.
- Live JOC software-reference path.
- Complete-file IAMF rendered-PCM reference lane.
- Removal or honest de-scoping of incomplete runtime adapters.
- Preservation of the merged OAR stereo and 5.1 object-render differential lanes.

## Outside software-only acceptance

Physical eARC capture, USB/TDM electrical timing, DAC loopback, acoustics, legitimate protected-service compatibility and certification remain separate physical/external gates. Passing software CI must never be described as proof of those items.
