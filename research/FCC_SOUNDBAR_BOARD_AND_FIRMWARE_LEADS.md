# FCC soundbar board and firmware leads

## Scope

This note records evidence-backed leads for identifying real-world eARC receiver hardware used in commercial soundbars.

## Verified findings

### Samsung HW-Q600C FCC package

The FCC filing for Samsung HW-Q600C includes two internal-photo exhibits. One public exhibit is 14 pages / about 3.1 MB and another later exhibit is 13 pages / about 3 MB. The filing also lists separate block-diagram, schematics, operational-description and parts-list exhibits, but those are metadata-only / confidential.

Implication: current FCC packages can confirm board partitioning and internal construction, but the public photo resolution may be insufficient to read HDMI/eARC IC markings directly.

### Public photos are not enough by themselves

The HW-Q600C internal photos establish that suitable teardown material exists, but do not yet provide a legible chip marking for the HDMI/eARC area. Aurora must not infer the receiver IC from package shape or board position alone.

### Firmware is a parallel evidence source

Samsung publishes large MAIN firmware packages for modern soundbars. The HW-Q990D support page currently exposes a 2026 MAIN firmware package of roughly 1.2 GB. Such packages may contain partition names, device-tree data, kernel modules, strings, update manifests or binary component names that can identify the audio SoC, HDMI/eARC controller or board architecture.

This is now a higher-value research path than relying only on low-resolution teardown photos.

## Research strategy

1. Acquire public internal-photo exhibits from FCC/device.report mirrors and inspect the HDMI connector area at native resolution.
2. Search spare-parts listings using visible Samsung PCB assembly numbers rather than consumer model names.
3. Analyze publicly downloadable soundbar firmware packages for strings and manifests that mention HDMI, eARC, ARC, IT6620, EP92, Explore, ITE, Lontium, Amlogic, MediaTek, Samsung audio SoCs or board codenames.
4. Treat confidential FCC schematic/parts-list entries as proof that documentation exists, not as evidence of a specific chip.
5. Reject any donor board unless the audio path can be established as eARC RX -> accessible I2S/TDM/PCM or as a Linux-controllable SoC capture interface.

## Current conclusion

No commercial soundbar donor board has yet been proven suitable for Aurora. The FCC route remains useful, but firmware analysis and spare-board PCB-number searches now have higher expected value than generic teardown-image searching.
