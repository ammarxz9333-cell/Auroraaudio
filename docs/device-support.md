# Platform support

Aurora does not have a canonical hardware device.

The supported software surfaces are defined by backends and CI evidence:

- **Linux**: primary development and validation environment; local audio uses the generic host-audio boundary.
- **Windows**: cross-platform build/test surface for the portable Rust software stack.
- **Simulation**: deterministic virtual audio devices and clocks for routing, drift, latency, and fault testing.

A future phone, SBC, MCU, HDMI/eARC frontend, USB bridge, DAC, amplifier, or speaker system may integrate through adapters. None of them is required by Aurora and none should be named as the project target.
