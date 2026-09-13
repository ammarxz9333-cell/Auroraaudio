#!/usr/bin/env python3
"""Capability contract for Aurora's executable Rust resilience simulation.

The actual clock-control and reconnect evidence is produced by
`crates/aurora-realtime-audio-sim/examples/resilience_evidence.rs` and gated by
`test-aurora-resilience-sim.sh`.  This module intentionally contains no second
Python implementation of those algorithms; it only exposes their capability
and profile names to the mandatory simulation-coverage registry.
"""

from __future__ import annotations

import json

SIMULATOR_CAPABILITIES = (
    "adaptive-clock-rate-correction",
    "device-reconnect-recovery",
)

FAULT_PROFILES = (
    "clock-correction-plus-250ppm",
    "clock-correction-minus-250ppm",
    "device-reconnect",
    "device-reconnect-exhaustion",
)


def describe() -> dict[str, object]:
    return {
        "schema_version": 1,
        "model": "aurora-resilience-rust-sim-v1",
        "capabilities": list(SIMULATOR_CAPABILITIES),
        "fault_profiles": list(FAULT_PROFILES),
        "implementation": "crates/aurora-realtime-audio-sim/examples/resilience_evidence.rs",
        "gate": "validation/virtual-hardware/test-aurora-resilience-sim.sh",
        "truth_boundary": (
            "hardware-independent virtual clock and device-lifecycle evidence using "
            "Aurora's real Rust DriftController, PPM estimator, RubatoAsrc and "
            "DuplexStateMachine; not physical clock, hotplug or backend evidence"
        ),
    }


def self_test() -> None:
    if len(set(SIMULATOR_CAPABILITIES)) != len(SIMULATOR_CAPABILITIES):
        raise AssertionError("duplicate resilience simulator capability")
    if len(set(FAULT_PROFILES)) != len(FAULT_PROFILES):
        raise AssertionError("duplicate resilience fault profile")
    if not SIMULATOR_CAPABILITIES or not FAULT_PROFILES:
        raise AssertionError("resilience simulator contract is empty")
    print(
        "AURORA-RESILIENCE-SIM-CONTRACT-PASS "
        f"capabilities={len(SIMULATOR_CAPABILITIES)} profiles={len(FAULT_PROFILES)}"
    )


if __name__ == "__main__":
    self_test()
    print(json.dumps(describe(), indent=2, sort_keys=True))
