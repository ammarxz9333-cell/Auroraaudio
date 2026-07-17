# Fleet Management

## Status

- `authorization_state`: `PROPOSED`
- `execution_state`: `NOT_STARTED`
- scope: future managed Aurora endpoints; no implementation authorized

## Objective

Aurora shall support secure, observable lifecycle management for Raspberry Pi-class devices and later supported endpoints without coupling the real-time audio path to a specific commercial fleet platform.

## Device identity and enrollment

Each endpoint shall have:

- a stable device identifier distinct from hostname and network address;
- authenticated enrollment with revocable credentials;
- declared hardware model, audio interface, software version, and capability set;
- an assigned logical role such as hub, front-left, subwoofer, surround, height, or generic zone endpoint;
- a human-readable location and zone assignment;
- explicit ownership and trust state.

Unknown or unauthenticated devices must not join an active audio topology.

## Desired and observed state

Fleet control shall separate:

- **desired state**: approved version, configuration, role, zone, and policy;
- **observed state**: current version, runtime status, health, connectivity, and drift;
- **reconciliation state**: pending, converged, blocked, degraded, or rollback-required.

The control plane must never directly execute time-critical DSP work.

## Required capabilities

- zero-touch or guided provisioning;
- inventory and capability discovery;
- role and zone assignment;
- configuration distribution with schema validation;
- remote health summaries and bounded diagnostic logs;
- version visibility and configuration-drift detection;
- maintenance mode and safe endpoint removal;
- staged deployment groups and canary cohorts;
- credential rotation and device revocation;
- audit records for every administrative mutation.

## Failure semantics

Fleet unavailability must not corrupt the active signal path. Endpoints shall continue according to a documented offline policy using the last accepted configuration, or enter a safe muted state where continuation would be unsafe.

A disconnected endpoint must be classified distinctly from a silent, unhealthy, unsynchronized, or intentionally disabled endpoint.

## Security boundaries

- least-privilege credentials per device;
- signed configuration and update metadata;
- encrypted management traffic;
- no unauthenticated remote shell as a product dependency;
- bounded log collection that excludes secrets and user audio content by default;
- explicit administrative authorization for destructive actions.

## Validation requirements

The Validation Lab shall test enrollment failure, duplicate identity, revoked credentials, configuration drift, offline operation, controller loss, partial fleet updates, stale desired state, and endpoint recovery.
