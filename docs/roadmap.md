# Aurora software roadmap

## P0 — immersive proof

- complete reproducible E-AC-3 JOC -> IEC61937 -> real object metadata -> 7.1.4 software rendering;
- keep a plain E-AC-3 negative control so channel audio is never misreported as objects;
- keep synthetic surround-upmix explicitly separate from object decoding.

## P1 — renderer and realtime integration

- converge the object/channel decoder boundary on Aurora-owned types;
- complete generic 3D loudspeaker rendering contracts and evidence;
- harden realtime buffering, ASRC, drift control, recovery, and latency accounting;
- keep renderer/DSP steady-state paths allocation-free.

## P2 — open immersive ecosystem

- advance IAMF integration where the upstream/licensing state permits;
- add optional SOFA/HRIR support for real HRTF rendering;
- improve external renderer/DSP adapters without coupling the core to them.

## P3 — distribution and platform adapters

- add optional network/multiroom or platform-specific adapters only after the core software contracts are stable;
- keep all physical-device work outside the canonical architecture and require measured evidence for physical claims.

No milestone requires a particular phone, board, MCU, eARC receiver, DAC, amplifier, or speaker product.
