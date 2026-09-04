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
AURORA_REALTIME_MCU_VENDOR_STACK
AURORA_REALTIME_MCU_VENDOR_STACK_VERSION
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

They must not encode the active part number, vendor family, or superseded target names in paths or conditionals.

Rust/C realtime protocol code must depend on Aurora protocol/capability contracts rather than a concrete MCU model wherever practical.

## Changing the MCU

A normal capability-compatible MCU change begins with this manifest, followed by target-specific validation.

A replacement is accepted only if CI confirms the declared capabilities and the target-specific pinmux/HAL/physical gates are re-run. Changing a manifest value never converts an unverified hardware claim into measured evidence.

Portable Aurora firmware remains at:

```text
firmware/realtime-mcu
```

and `AURORA_REALTIME_MCU_SOURCE_DIR` must resolve to that target-neutral source tree unless a future architecture change explicitly introduces a separate portable implementation contract. Concrete part names must not be reintroduced as source-directory names.

Target-vendor bindings, generated constants and board-specific implementation details must remain behind the realtime-MCU HAL/manifest boundary rather than renaming the portable API.

## Directory-name rule

Current portable MCU source, tests and application APIs use only target-neutral names such as:

```text
firmware/realtime-mcu
aurora_realtime_mcu_app_*
aurora_realtime_mcu_hal_*
AURORA_REALTIME_MCU_*
```

A future part replacement must not require mechanical renaming of these public/internal architecture surfaces. Concrete vendor/family/part/package identity belongs in the hardware-target manifest and target-specific proof material only.

## Safety

Hardware-target switching is always fail-closed:

- unsupported required capability => CI failure;
- unverified pinmux => no physical-ready claim;
- missing HAL => no firmware-ready claim;
- missing physical measurement => no hardware acceptance claim.

The selected MCU never owns Atmos/JOC decoding or object rendering. Those remain on the Galaxy S6 software side; the realtime MCU owns deterministic transport, clocked capture/playback, DMA buffering, telemetry and hardware mute/safety.
