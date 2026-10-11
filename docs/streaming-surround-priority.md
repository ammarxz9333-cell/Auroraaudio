# Streaming-app surround as Aurora's first priority

Status (2026-10-11): software-only encoded-channel-bed path added. **No working Netflix/Disney+/Prime Video physical eARC validation is claimed.** Physical 12-channel DAC and wireless speaker synchronization remain independent acceptance gates.

## Minimum output

Aurora's fixed output layout is **7.1.4 (12 channels / S32_LE / 48 kHz)**, exceeding the requested 7.1.2 minimum. Canonical slot order: FL, FR, FC, LFE, SL, SR, SBL, SBR, TFL, TFR, TRL, TRR. A 7.1.4 PCM container or active synthetic height channels do **not** establish authentic Atmos metadata. Each source's real available channels must be separately declared.

## Legitimate streaming-service path

~~~text
Netflix / Disney+ / Prime Video / Fire TV app
  -> authorized TV/streaming device HDMI output
  -> TV's legitimate eARC output (format and passthrough depend on its settings)
  -> existing Lindy 38368 + Aurora-specific SiI9437 test tap
  -> captured S32_LE, 2ch, 192 kHz
  -> existing Aurora IEC61937 extraction
  -> media format selection
       E-AC-3 JOC native-object lane: existing Harletty / Aurora VBAP3D
       AC-3 / E-AC-3 channel-bed lane: this PR's FFmpeg decoder + channel upmix
       Dolby MAT / PCM / AC-4: NOT yet supported by this IEC61937 lane
  -> canonical Aurora 7.1.4 PCM
  -> existing adaptive FIFO + ASRC -> physical 12ch DAC
~~~

Do not assume every source outputs DD+ JOC. The Amazon Fire TV Stick 4K Max (2nd gen) includes Dolby MAT transcoding support on some streaming paths. A MAT-compatible captured ingress is a separate requirement, not something FFmpeg's IEC61937 E-AC-3 demuxer magically decodes.

## New channel-bed backend

Run against a pre-existing *authorized* IEC61937 stream:

~~~bash
python3 validation/streaming/aurora_streaming_bed.py --require-surround-input \
    < capture.spdif > aurora-7.1.4.s32
~~~

The script buffers a bounded IEC preamble, uses FFprobe to identify **actual** codec and decoded channel layout, and permits only AC-3/E-AC-3 stereo, 5.1(side), 5.1(back), or 7.1. It rejects unknown formats rather than relabel them as 7.1.4. In strict mode, stereo input fails; without the strict option, stereo is explicitly synthesized into 12 channels with no Dolby object claim.

For 5.1 it preserves FL/FR/FC/LFE and the native surround pair and creates low-level, stereo-difference-derived back/height ambience. For 7.1 it preserves all eight original channels and synthesizes only height ambience. The center and LFE never feed synthetic heights. Upmix cannot create missing native surround information. All modes output exactly 12 interleaved signed 32-bit channels at 48 kHz to match Aurora's live 7.1.4 output contract.

Run local software codec verification:

~~~bash
python3 validation/streaming/test_streaming_bed.py
~~~

This test **encodes** actual AC-3 and E-AC-3 5.1 stimuli via FFmpeg, sends real IEC61937 carriers through the new decoder, checks 12-channel format, checks energy on the six known original channels and synthetic back/heights, checks strict stereo rejection and explicit stereo upmix, and rejects malformed input. It is **not** a protected-service test or a continuous hardware timing measurement.

## Existing physical live-eARC runner integration

The existing Aurora JOC runner now accepts an explicit `--decoder-mode channel-bed` switch, while keeping `joc-native` as the default. Example:

~~~bash
bash validation/physical/run-live-earc-vbap.sh \
  --capture-device 'hw:CARD,DEV' \
  --decoder-mode channel-bed \
  --output-device 'plughw:CARD,DEV' --seconds 85
~~~

This reuses Aurora's IEC capture conversion and adaptive 12-channel PCM output. The actual ALSA device must first be detected on the selected physical host; these names are placeholders. It is not a one-command proof of Netflix compatibility without actual hardware/recording evidence.

## What is not yet established

1. Continuous format changes between AC-3, DD+, JOC, MAT and stereo, especially when TV/audio settings change mid-playback. This initial decoder probes the starting layout and **does not** prove seamless format-change renegotiation. Reject/re-arm until physically verified, rather than silently lose channels.
2. Native JOC from an actual Netflix/Disney+/Prime title across the selected authorized TV eARC tap. Existing Aurora JOC proof is fixture-based.
3. Dolby MAT / multichannel LPCM intake, AAC/AC-4 input and 7.1.2+ native height metadata. All must have independently evidenced admission and format selection; they are not ordinary IEC E-AC-3 0x15.
4. Long-duration physical 12-channel DAC output, speaker synchronization, eARC reset/drop counters, latency and AVR-compatible sound at the seat.

## Hardware promotion gates

- Real authorized eARC capture including input-type / clock evidence.
- At least one title on each intended service with actual observed incoming codec and channels.
- Verified DD+ 5.1 -> 7.1.4 synthetic output; verified JOC object lane -> 7.1.4 native render, keeping respective labels.
- PCM/MAT/AC-4 tested separately; never claim compatibility simply because the Fire TV itself supports those input codecs.
- Real DAC/electrical channel probes for all output slots, correct side/rear/height mapping, and continuous hours without XRUNs or clock drift.
- Service playback and UI switching/seek/pause tests on real hardware.

Mobile calibration and motorized auto-aim remain lower priority until this path is proven.
