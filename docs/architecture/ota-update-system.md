# OTA Update System

## Status

- `authorization_state`: `PROPOSED`
- `execution_state`: `NOT_STARTED`

## Objective

Aurora shall support authenticated, staged, recoverable software and configuration updates for managed endpoints while protecting the real-time audio path and preserving a known-good rollback target.

## Update package requirements

Every release artifact shall include:

- immutable version and content hash;
- target hardware and compatibility metadata;
- signed manifest and payload;
- minimum bootloader/runtime requirements;
- configuration-schema compatibility;
- migration and rollback declarations;
- release-channel designation;
- provenance linking the artifact to a repository revision and CI evidence.

Unsigned, incompatible, incomplete, or untraceable artifacts must be rejected.

## Deployment strategy

The updater shall support:

1. preflight validation;
2. artifact download without disrupting active audio where possible;
3. integrity and signature verification;
4. staged canary deployment;
5. health observation during a defined bake period;
6. progressive cohort expansion;
7. automatic halt on mandatory health regressions;
8. rollback to the last accepted version;
9. final fleet convergence reporting.

Updates affecting active topology shall coordinate endpoint order to avoid uncontrolled partial-system behavior.

## Failure and recovery semantics

- interrupted downloads resume or restart safely;
- power loss must not leave an endpoint without a bootable accepted image;
- failed boot or failed post-update health checks trigger rollback;
- incompatible configuration migration fails closed;
- a partially updated fleet remains visible and explicitly classified;
- rollback failure places the endpoint in a recoverable maintenance state.

## Release channels

Suggested channels:

- `development`;
- `canary`;
- `candidate`;
- `stable`.

Promotion requires accepted Validation Lab evidence appropriate to the channel. Physical-device promotion gates remain separate from simulated evidence.

## Security and audit

All deployments, cancellations, promotions, and rollbacks shall be authenticated and audited. Update credentials must be separable from audio-stream credentials. The system shall defend against replay, downgrade, manifest substitution, and unauthorized channel changes.

## Validation

Automated tests shall cover corrupted payloads, invalid signatures, incompatible targets, storage exhaustion, power interruption, failed migration, failed health checks, partial fleet deployment, controller loss, rollback, and recovery.
