# ADR 0007: Bounded PI Drift Controller

- Status: Accepted for live-duplex validation
- Date: 2026-07-16

Use a slow PI controller driven by ring fill, with a nominal rate ratio, +/-500
ppm clamp, anti-windup, 2 ppm maximum movement per update, numeric saturation,
and a fatal prolonged-saturation threshold. One ratio applies to every channel.

Ring fill is a direct scheduling observable and avoids pretending CPAL exposes a
hardware clock position. The controller is intentionally conservative; future
hardware captures may justify gain changes. Gains are public configuration values,
not undocumented constants.

