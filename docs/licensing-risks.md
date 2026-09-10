# Licensing Risks

## Current Engineering Rules

- Keep Aurora-owned transport, decoder, renderer, DSP, and hardware boundaries explicit. Do not copy proprietary Dolby/DTS/HDMI/HDCP SDK source, firmware, keys, certificates, or confidential specifications into the repository.
- Direct eARC/IEC61937 ingest is now an implemented engineering path, not a prohibited Phase 0 topic. The native production path is owned by `aurora-encoded-runtime` and Aurora's ALSA backend; `aurora-direct-earc-ingest` is a stdin-only normalization/evidence helper.
- Treat IEC61937 data type `0x15` only as E-AC-3 transport classification. It is not evidence of Dolby Atmos/JOC. An Atmos/JOC software claim requires successful JOC admission and successful renderer output; a physical-product claim additionally requires hardware acceptance evidence.
- Keep third-party codec/rendering implementations behind Aurora-owned interfaces and pin reviewed source revisions. Record redistributed/runtime dependencies and their notices in `THIRD_PARTY_LICENSES.md` / `THIRD_PARTY.md` as applicable.
- Do not add DRM circumvention, service-key extraction, protected-media decryption, HDCP bypass, or capture behavior intended to defeat a streaming provider's access controls. Aurora's eARC path starts at audio already exposed by the playback device/receiver interface.
- Do not infer commercial redistribution, trademark, patent, certification, or HDMI/Dolby program rights from an open-source software license. Those questions require separate product/legal review.

## Known Risk Areas

- AC-3, E-AC-3/JOC, object-audio rendering, and related technologies may be covered by patents, trademarks, certification requirements, or jurisdiction-specific obligations independent of source-code copyright licensing.
- HDMI/eARC hardware implementation may require adopter agreements, licensed IP, compliance testing, approved transmitter/receiver components, or other ecosystem obligations. Software-level IEC61937 handling does not establish hardware compliance.
- Third-party projects can change license terms, dependency graphs, or redistribution requirements between revisions. Aurora therefore pins reviewed revisions and must re-audit before updating them.
- FFmpeg and native/system libraries can have configuration-dependent licensing implications. The exact build and enabled components used for distribution must be reviewed, not merely the package name.
- Commercial distribution requires a separate legal/compliance review covering source licenses, notices, patents, trademarks, hardware certification, and target jurisdictions.

## Current Checkpoint Status

Aurora now contains a native direct-eARC encoded-audio path, IEC61937 parsing,
open AC-3/E-AC-3 decoding, JOC admission/render integration, canonical 7.1.4 DSP,
and native ALSA input/output plumbing. This is an engineering status statement,
not a declaration of Dolby/HDMI certification or commercial distribution rights.
Physical eARC/TDM interoperability and commercial JOC acceptance remain separate
hardware/product acceptance gates.
