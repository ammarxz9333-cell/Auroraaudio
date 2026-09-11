# Aurora vision

Aurora is an open immersive-audio software engine, not a hardware product definition.

The project exists to make spatial and multichannel audio processing inspectable, replaceable, testable, and portable. Its core must remain independent of any particular phone, SBC, processor, MCU, HDMI/eARC board, DAC, amplifier, or speaker product.

## Principles

1. **Hardware agnostic** — physical devices are optional adapters, never architectural requirements.
2. **Open interfaces** — Aurora-owned APIs separate decoding, scene representation, rendering, DSP, realtime scheduling, and I/O.
3. **Evidence before claims** — capabilities are promoted only when deterministic tests or measured evidence exist.
4. **No silent fallback** — object decoding, channel decoding, and synthetic upmixing remain explicitly distinct modes.
5. **Portable realtime core** — timing, buffering, drift correction, ASRC, and state machines stay reusable across operating systems and devices.
6. **Replaceable backends** — external projects may be integrated behind narrow boundaries without becoming the project identity.
7. **No device lock-in** — deleting or replacing a hardware adapter must not require rewriting the engine.

## Near-term software milestones

- complete reproducible E-AC-3 JOC -> object metadata -> 7.1.4 software validation;
- consolidate generic immersive input and renderer adapter contracts;
- keep deterministic simulation and renderer evidence green across Linux and Windows;
- improve realtime reliability and latency instrumentation on ordinary host audio devices;
- expand open immersive formats and renderer backends where licensing permits.

Physical product design is intentionally outside the core roadmap.
