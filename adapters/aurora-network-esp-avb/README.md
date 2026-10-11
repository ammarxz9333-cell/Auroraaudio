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

## PTP time and AECP/AEM control

The exact pinned esp_avb component already implements IEEE 1722.1
SET_CONTROL and GET_CONTROL. Aurora therefore uses the existing ATDECC/AECP
control plane rather than inventing a second UDP or P4-to-C6 RPC protocol.

Upstream control indices 0-2 remain untouched. Aurora reserves 3-7 for the
exact-pin endpoint patch:

- 3: handover epoch (UINT64)
- 4: rendered role lane 0 (UINT8)
- 5: rendered role lane 1 (UINT8, 0xff means unused)
- 6: absolute PTP activation time in nanoseconds (UINT64)
- 7: endpoint command MUTE/PREPARE/ARM/COMMIT/STOP (UINT8)

The host never sends an Aurora frame number directly to a physical endpoint.
A PtpMediaAnchor maps the shared 48 kHz media timeline to absolute PTP time;
for example, frame 96000 maps to 2,000,000,000 ns when frame 48000 is anchored
at 1,000,000,000 ns. Both detached pods therefore receive the same absolute
activation time even though they do not share the host frame counter origin.

Each endpoint command is staged as EPOCH first, command-specific values next,
and COMMAND last. A future AVDECC sender must require a successful AECP
response for every SET_CONTROL write and fail the whole handover if any write
is not acknowledged.

Run:

```bash
cargo run --release \
  --manifest-path adapters/aurora-network-esp-avb/Cargo.toml \
  --example aem_control_probe
```

Expected evidence:

```text
AURORA-ESP-AVB-AEM-CONTROL-PASS modules=2 envelopes=8 writes=22 start_ptp_ns=2000000000 controls=3-7 transport=aecp-aem
```

This is still software contract evidence. The pinned firmware must be patched
and built with descriptors 3-7 before physical endpoint control is considered
proven.

## Truth boundary

This is host-side software contract evidence. It does not prove that the pinned
ESP32-P4/C6 firmware currently receives these envelopes over a physical RPC
channel, nor does it prove physical PTP lock, RF performance, sample alignment,
DAC/Class-D hardware, battery/charging behavior, acoustic skew, or Dolby
certification.
