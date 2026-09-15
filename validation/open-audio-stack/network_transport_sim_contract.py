#!/usr/bin/env python3
"""Export the Rust network-transport simulator contract to Aurora coverage validation.

The executable implementation lives in `aurora-realtime-audio-sim` and
`aurora-realtime-engine`; this module only exposes its capability and unique
fault-profile names to the mandatory repository-wide simulation catalogue.
The Open Audio Stack CI executes the Rust tests that back these declarations.
"""

SIMULATOR_CAPABILITIES = ("network-audio-transport-contract",)

# `none` is already exported by Aurora's full-system simulator as the shared
# healthy baseline. These network-specific profile names remain unique across
# simulator contract sources; `healthy` names the explicit network lifecycle
# case exercised by the Rust simulator tests.
FAULT_PROFILES = (
    "healthy",
    "overflow",
    "format-drift",
    "timestamp-discontinuity",
)
