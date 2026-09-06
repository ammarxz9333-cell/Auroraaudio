# Upstream Component Plan

Aurora should integrate proven upstream components behind narrow adapters instead
of implementing codecs, room-correction engines, streaming receivers, and
multi-room transports from scratch. The machine-readable decision record is
`config/external-components-v1.json`; CI validates it with
`scripts/check_external_components.py`.

## Selected path

1. Use the system FFmpeg process for supported AC-3, E-AC-3, TrueHD, and DTS
   channel-bed decoding, resampling, and format conversion. Output crosses into
   Aurora only as canonical PCM plus explicit channel labels. This does not
   recover Dolby JOC or DTS:X object metadata.
2. Keep Aurora's existing CamillaDSP adapter for offline filter generation and
   validation. Promote it to live use only after target-device process, buffer,
   underrun, and physical latency tests pass.
3. Keep Omniphony and Harletty confined to the S6 experimental appliance lane.
   Their presence does not establish decoder correctness, licensed Dolby
   compatibility, streaming-service access, or production readiness.
4. Evaluate libiamf separately as the preferred open immersive-object path.
   It must not be coupled to proprietary codec claims.

## Useful, but not on the critical path

- libmysofa is appropriate for a later headphone/SOFA-HRIR adapter. It does not
  improve the primary loudspeaker renderer by itself.
- RNNoise can provide optional speech denoising on a worker path. It must not be
  called directly from Aurora's real-time callback until allocation, blocking,
  and fixed-quantum behavior are proven.
- Shairport Sync and spotifyd are reasonable external receiver candidates. They
  are separate service integrations, not codec bypasses.
- Snapcast is a useful multi-room reference or external service. It should not
  replace Aurora's clock/evidence work without measured target validation.

## Deliberately excluded

- EDID impersonation, DRM interception, Widevine bypass, and unlicensed claims.
- Hand-written fake decoders or synthetic object metadata presented as Atmos.
- Vendoring GPL or patent-sensitive codec implementations into Aurora core.
- Marking any selected component production-ready before evidence and release
  licensing review exist.

## Promotion gates

An external component moves from evaluation to live deployment only when its
version is pinned, license posture is recorded, adapter failure is fail-closed,
canonical PCM/channel mapping is tested, and the intended hardware path has
captured evidence. Software-only tests must remain labelled software validation.
