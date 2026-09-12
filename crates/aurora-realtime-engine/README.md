# Aurora realtime-engine boundary

`aurora-realtime-engine` is the steady-state audio execution boundary. It consumes **prepared, typed** renderer and realtime-DSP components after control-plane validation and materialization.

Configuration envelopes such as `ComponentReference`, component-specific JSON payloads, and renderer/backend registries belong to the control plane and must not cross into this crate's callback path. Backend selection, contract/version negotiation, capability validation, and device activation are resolved before realtime processing begins.

The callback path therefore remains implementation-agnostic and realtime-safe: no configuration parsing, registry lookup, filesystem/process access, logging/formatting, locks, or post-preparation allocation may be introduced here.

This boundary is architectural evidence only; it does not claim that a configured component was physically activated, negotiated with hardware, measured, or certified.
