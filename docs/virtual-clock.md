# Virtual Clock

The virtual scheduler has independent input and output domains. Time is stored
as integer picosecond ticks. A device period is derived from nominal sample
rate, callback frames, and signed ppm offset. This avoids wall-clock sleeps and
makes accelerated runs deterministic.

A small seeded xorshift generator selects bounded callback sizes, jitter, and
loopback noise. Equal seeds and configuration produce equal callback events and
checksums. Different seeds alter stochastic events without changing bounds.

Supported callback behavior includes fixed 64/128/256/512 blocks, alternating
and random bounded blocks, one-frame partial callbacks, oversized callbacks,
bursts, starvation windows, missing callbacks, and scheduled stalls. Scheduler
steady state performs no allocation.
