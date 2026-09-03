# Aurora hardware target contract

Status: **machine-readable selection contract; not physical acceptance evidence**.

## Single source of truth

The active realtime MCU is selected only in:

```text
config/aurora-hardware-target.env
```

No build script, CI command, runtime service, protocol implementation, or hardware-facing document may require the concrete MCU part number to be duplicated elsewhere.

The stable symbolic role is:

```text
AURORA_REALTIME_MCU_ROLE=aurora-realtime-mcu
```

The currently selected vendor/family/part/package are data owned by the manifest. They are not architectural API names.

## Required symbolic fields

Every target manifest must define at least:

```text
AURORA_REALTIME_MCU_ROLE
AURORA_REALTIME_MCU_VENDOR
AURORA_REALTIME_MCU_FAMILY
AURORA_REALTIME_MCU_PART
AURORA_REALTIME_MCU_PACKAGE
AURORA_REALTIME_MCU_SOURCE_DIR
```

and explicit capability/status fields for:

- USB 2.0 High-Speed host support;
- external HS PHY mode;
- audio capture peripheral support;
- multichannel/TDM output support;
- DMA availability;
- sample rate;
- channel count;
- transport period;
- pinmux validation state;
- HAL implementation state;
- physical measurement state.

## Consumer rule

Shell/CI consumers must load the manifest and use symbolic variables, for example:

```sh
. config/aurora-hardware-target.env
MCU="$AURORA_REALTIME_MCU_SOURCE_DIR"
cc -I"$MCU/include" ...
```

They must not encode the active part number in paths or conditionals.

Rust/C realtime protocol code must depend on Aurora protocol/capability contracts rather than a concrete MCU model wherever practical.

## Changing the MCU

A normal pin-compatible/capability-compatible MCU change should require edits to this manifest only, followed by validation.

A replacement is accepted only if CI confirms the declared capabilities and the target-specific pinmux/HAL/physical gates are re-run. Changing a manifest value never converts an unverified hardware claim into measured evidence.

If a replacement requires different portable firmware source, change `AURORA_REALTIME_MCU_SOURCE_DIR` in the same manifest. Consumers still remain unchanged.

## Directory-name rule

Historical directory names are not target truth. The current portable MCU source directory may retain an older family/part label until a mechanical rename is performed, but all build and CI consumers must resolve it through `AURORA_REALTIME_MCU_SOURCE_DIR`.

A future mechanical rename to a fully generic directory must not alter the public protocol or hardware-selection contract.

## Safety

Hardware-target switching is always fail-closed:

- unsupported required capability => CI failure;
- unverified pinmux => no physical-ready claim;
- missing HAL => no firmware-ready claim;
- missing physical measurement => no hardware acceptance claim.

The selected MCU never owns Atmos/JOC decoding or object rendering. Those remain on the Galaxy S6 software side; the realtime MCU owns deterministic transport, clocked capture/playback, DMA buffering, telemetry and hardware mute/safety.
