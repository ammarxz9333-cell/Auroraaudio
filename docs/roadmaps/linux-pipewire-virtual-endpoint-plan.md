# Linux PipeWire Virtual Endpoint Plan

## Purpose

Use `Berny23/virtual-surround-manager` only as an endpoint-orchestration reference for Aurora's Linux integration. Aurora retains its own renderer, graph model, validation, and platform-neutral DSP contracts.

## Adopt

- Virtual multichannel sink for applications.
- Separate Aurora processing boundary and downstream output node.
- Auto-route only streams that follow the system default sink.
- Respect explicit application device assignments and user exclusions.
- Runtime enable/disable, output-device changes, hotplug recovery, and PipeWire/WirePlumber restart recovery.
- Stable metadata for chaining optional stereo processing after Aurora.
- Per-decision routing diagnostics and deterministic restoration.

## Adapt

- Implement the controller in Aurora's Rust platform layer.
- Generate endpoint topology from validated channel-layout contracts.
- Keep all convolution and rendering inside Aurora rather than PipeWire filter-graph strings.
- Admit HeSuVi-compatible HRIR presets only through the dataset registry and validation gate.
- Define explicit LFE policy instead of mapping LFE silently to center.
- Keep endpoint lifecycle independent from GUI frameworks.

## Reject

- Hardcoded HRIR WAV channel indices in core DSP.
- Fixed 5.1/7.1-only architecture.
- Global hijacking of all output streams.
- Qt coupling.
- EasyEffects dependency.
- PipeWire graph text as Aurora's renderer.
- Silent LFE-to-center behavior.

## Execution order

1. Finalize platform-neutral endpoint and channel-layout contracts.
2. Implement PipeWire registry and metadata observation.
3. Implement conservative stream-routing policy.
4. Create multichannel capture and processed-output nodes.
5. Add reversible lifecycle state machine.
6. Add physical-device hotplug and daemon-restart recovery.
7. Add downstream stereo-effects interoperability fixture.
8. Add validated HRIR preset selection through issues #52 and #53.
9. Add explicit LFE policies and regression tests.
10. Publish routing and recovery artifacts through the simulation/assurance reporting path.

## Acceptance

- No routing loops or duplicate capture.
- Explicitly assigned streams remain untouched.
- Enable/disable is reversible while audio is active.
- Default-device changes and device hotplug recover deterministically.
- PipeWire restart does not require manual reconfiguration.
- 5.1 and 7.1 fixtures pass with declared channel maps.
- Unknown layouts fail safely.
- Every route, release, and restoration decision is diagnosable.
- Core DSP remains platform-neutral.

Tracking: issue #56.
