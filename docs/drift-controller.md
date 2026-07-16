# Drift Controller

The adaptive duplex consumer owns one conservative PI controller. Its ratio is
shared by every channel. Positive ring-fill error lowers the output/input ratio,
causing the fixed-output resampler to consume more input frames.

Default policy:

- nominal ratio: negotiated output rate divided by negotiated input rate
- adaptive clamp: +/-500 ppm around nominal
- proportional gain: 500 ppm at one target-fill error
- integral gain: 0.2 ppm per second at one target-fill error
- maximum movement: 2 ppm per output block
- anti-windup: freeze integration while saturation pushes farther out
- fatal mismatch: 1,500 consecutive saturated updates

Numeric status includes current/minimum/maximum ratio, correction ppm, fill
error, total saturation, consecutive saturation, and controller fault. Callback
code does not log.

One-second hardware-independent simulations at 48 kHz cover +/-10, 25, 50, 100,
and 250 ppm for 10 minutes, one hour, and eight hours. A target fill of 2,048 and
capacity of 8,192 remain bounded. At 10 minutes the final corrections are within
approximately 8 ppm of the injected clock error even at +/-250 ppm; longer runs
converge toward the opposing ppm correction without unbounded fill growth.

