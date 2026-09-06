# Explicit surround-upmix adapter

Status: experimental channel decoding and synthetic height effects, tested on a
Linux host with generated compressed audio. Not Dolby Atmos/JOC object decoding,
not physical live-service acceptance, and not a validated Samsung Q995 equivalent.

## Two modes

`AURORA_DECODE_MODE=objects` retains the external Harletty/Omniphony path.
`AURORA_DECODE_MODE=surround-upmix` selects the external FFmpeg adapter.
Unknown modes are rejected. There is no silent fallback from objects to upmix.
The adapter writes `objects_decoded=false heights=synthetic` to stderr at startup.

The alternative receives canonical IEC61937 from the existing broker and lets
FFmpeg decode its channel bed. It normalizes the bed to 7.1 at 48 kHz, retains
those eight channels, and produces four additional ambience channels from
front and surround left/right differences. A 250 Hz high-pass, 7 kHz low-pass,
all-pass and unequal 11/17/23/29 ms effect delays shape the generated heights.
These delays are intentional effects, not measured system latency.

Center and LFE do not feed the effect. Equal left/right mono content cancels
from the height difference signal. True 5.1 inputs use FFmpeg's standard 7.1
rematrixing before this operation; additional bed channels are not newly decoded
independent channels. The effect cannot infer original object positions, guarantee
correct elevation, or substitute for room/speaker measurements. Its coefficients
are an experimental starting point, not acoustically tuned product settings.

Output order is `FL FR C LFE BL BR SL SR TFL TFR TBL TBR`, float32 little endian,
48 kHz. The existing Aurora postprocessor still owns crossover, calibration,
ASRC, gain/mute and limiting. The managed source gate and CONFIG gate remain in
the path. The shell wrapper uses `exec`, so the broker owns the actual FFmpeg
process and can stop it during disconnect/restart without a shell child pipeline.

## Running on an assembled appliance

Select these values in `/etc/aurora/aurora.env`, then restart the live-ingest service:

```sh
AURORA_DECODE_MODE=surround-upmix
AURORA_SURROUND_UPMIX_BIN=/usr/local/sbin/aurora-surround-upmix
AURORA_FFMPEG_BIN=/usr/bin/ffmpeg
```

The native builder stages the wrapper, and rootfs assembly installs it. FFmpeg
was already a rootfs dependency. The service checks the selected mode's runtime
dependencies, so surround-upmix does not require a loadable Harletty bridge at
startup. The standard image builder still builds/stages both adapter options.
No image was built or flashed during this change.

For a Linux host test independent of the hardware and broker socket:

```sh
python3 platform/s6/surround-upmix/test_surround_upmix.py
sh platform/s6/surround-upmix/aurora-surround-upmix.sh < captured.spdif > rendered.f32
```

The second command expects a canonical IEC61937 capture, not an MP4, raw E-AC-3
file, raw LPCM carrier or encrypted service download. Do not feed the resulting
12-channel file directly to a stereo device. Its channel routing and playback
gain must be handled by the intended multichannel output pipeline.

For actual streaming, the TV/box still handles service playback and supplies the
HDMI/eARC audio; a functioning receiver/capture front-end is required. This
adapter does not access service accounts, remove DRM, negotiate HDMI/eARC, or
make an S6 USB port into an HDMI input. A codec transition must be flagged as a
discontinuity by the upstream capture path so the existing broker restarts it.
Dolby MAT and raw LPCM are outside this adapter's tested input contract.

## Evidence on 2026-09-06

Using local FFmpeg 6.1.1:

- Generated a real E-AC-3 5.1 bitstream and muxed it into IEC61937 with FFmpeg.
- The adapter produced 49,152 frames of 12-channel PCM for the short fixture.
- Its first eight channels matched an independent FFmpeg 7.1 decode within
  1e-6 absolute sample error, preserving frame count and channel order.
- Distinct front/surround tones produced activity in all four synthetic heights.
- Center-only, LFE-only and equal-channel AC-3 stereo fixtures left heights silent
  within the same tolerance. All output samples were finite.
- A pipe test received PCM while input remained open: no EOF requirement.
- Invalid input failed with no PCM output.
- A 10.016-second fixture completed in 0.188 seconds on this host (53.3x offline
  throughput). This is not S6 throughput, a live scheduling guarantee, or a
  physical round-trip latency measurement.
- Broker warnings-as-errors build and existing C duplex-pressure tests passed.

Socket-based full-broker integration remains unavailable in this environment
(AF_UNIX SOCK_SEQPACKET returns EPERM). Cargo is absent, so Rust workspace gates
could not run. The true object adapter binary download was blocked; no real JOC
decoding test was completed. No native Alpine/S6 build, HDMI/eARC capture,
speaker audition, physical latency measurement or service playback was tested.

This supplies a tested software alternative for decoding the channel bed and
creating height effects. Only the separate physical live-streaming acceptance
contract can establish service compatibility; an acoustic comparison is still
required to evaluate similarity to Samsung Q995.
