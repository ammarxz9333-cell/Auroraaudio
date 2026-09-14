# Next work after software completion

After PR #159 merges, new validation work must move to a separate branch rather than expanding the software-completion PR.

Execution order follows `docs/pre-hardware-roadmap-v5.md`:

1. add JOCForge as a pinned fixture/conformance generator, not a runtime encoder;
2. expand Harletty/OpenJOC/JOCForge differential and malformed-stream coverage;
3. add the independent IAMF validation lane around libiamf, AOMedia iamf-tools, and OAR;
4. run the FFmpeg compatibility matrix before any baseline upgrade;
5. continue renderer/layout, room-correction, network, and hardware-reference phases only behind their documented gates.

No hardware is selected by PR #159. No protected-service compatibility is claimed from simulation, and Aurora must not rely on DRM/HDCP circumvention or falsified certification/device state.
