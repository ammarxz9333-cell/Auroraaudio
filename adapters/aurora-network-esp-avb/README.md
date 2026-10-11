# Aurora ESP-AVB adapter

This adapter keeps ESP-AVB media transport and detachable-speaker control behind
Aurora-owned contracts.

## Detachable control bridge

`compile_handover_control` converts the modular speaker fabric's atomic
handover into endpoint-addressed control envelopes:

```text
Aurora Fabric Handover
MUTE all changed modules
PREPARE route + roles
ARM shared future media frame
COMMIT
        |
        v
ESP-AVB control bridge
        |
        +-- module_id
        +-- endpoint_id
        +-- stream_id
        +-- PTP-only route validation
        +-- max 2 rendered lanes / endpoint
        |
        v
future board-specific ESP control transport
```

The bridge preserves global command order and fails closed on stale/mismatched
epochs, missing bindings, duplicate endpoint/stream identities, an inconsistent
start frame, unsupported lane count, DockBus, AdaptivePeer, or a wired/wireless
medium mismatch.

The current medium contract is deliberately strict:

- `WiredNetwork + Ptp` -> `WiredEsp32P4`
- `WirelessNetwork + Ptp` -> `WirelessEsp32C6`
- DockBus is not sent through ESP-AVB network control.
- AdaptivePeer is not accepted by this PTP-only adapter.

Run:

```bash
cargo run --release \
  --manifest-path adapters/aurora-network-esp-avb/Cargo.toml \
  --example detachable_control_probe
```

Expected evidence:

```text
AURORA-ESP-AVB-DETACHABLE-CONTROL-PASS messages=8 modules=2 epoch=42 start_frame=96000 lanes_per_pod=2 clock=ptp
```

## Truth boundary

This is host-side software contract evidence. It does not prove that the pinned
ESP32-P4/C6 firmware currently receives these envelopes over a physical RPC
channel, nor does it prove physical PTP lock, RF performance, sample alignment,
DAC/Class-D hardware, battery/charging behavior, acoustic skew, or Dolby
certification.
