# TV audio boundary to speaker loads

Run:

```powershell
python C:\Users\ammar\Auroraaudio-latest\validation\simics\run_tv_to_speakers.py
```

Each invocation uses a fresh UUID output directory under `artifacts/tv-to-speakers`.
The canonical `tv-to-speakers.json` records each process exit, source and binary
hashes, TV/DMA timing, new decoder pacing evidence and speaker-load output.
Failed phases stop the chain and save a failed report. Negative profiles must
produce their exact expected exit and explicit failure evidence.

## Executed path

1. Verify the official Dolby archive and original E-AC-3 identity; remove exactly
   its known malformed first 2560-byte AU, verify the pinned suffix, wrap DD+ in
   IEC61937 with FFmpeg. No prior PCM output is reused.
2. A Simics Python TV audio-boundary model transfers 2360 bursts, 24576 bytes each,
   at 32 ms intervals into vacuum RAM. The receiver reads RAM using DMA-sized
   transactions, checks headers/length/padding and reconstructs the exact payload.
3. Execute the real Harletty telemetry binary and Omniphony 7.1.4 renderer on that
   received stream. Execute a separate full media-paced render and Aurora's
   moving-object evidence analyzer. Existing Windows build products are reused
   with SHA-256 identities recorded; external dependencies are not rebuilt.
4. Execute the real Aurora `SpeakerPostProcessor` binary on the new PCM. Require
   preserved frame count and changed output identity. Run the existing healthy
   virtual transport test and all nine existing transport faults.
5. Run the new DSP PCM through Simics audio MMIO/DMA and MCHStreamer TDM16 modeled as two parallel TDM8 lanes. Feed each resulting
   period directly to the ideal DAC/amplifier/load model. Require finite in-range
   samples, zero padding, activity on all twelve loads and exact frame accounting.
   Single-channel impulses verify routing to each labelled load. Fault tests
   cover load disconnection, overrange, NaN, padding and malformed data; faults
   latch and mute. Additional MMIO/DMA tests remain mandatory.

Speaker roles: FL FR C LFE BL BR SL SR TFL TFR TBL TBR. The load model assumes
symmetric signed 24-bit quantization, unity gain, 2 V peak full-scale and 8 ohms
per load. Reported volts/watts are calculations under those assumptions, not
measurements or speaker acoustic performance. The two TDM8 lanes still transport abstract
float32 words into the conversion model, not a physical DAC wire protocol.

## What this does not establish

The stages exchange files; they are not simultaneously coupled in one real-time
clock domain. The host executes the decoder/render/DSP tools, not a complete
Aurora guest OS/runtime inside Simics. The TV is an audio-boundary model, not
Netflix-capable firmware. eARC electrical discovery, capability signalling,
HDCP, DRM, TV application behavior and actual Netflix delivery are not modeled.

The user confirmed no physical devices are available. On this laptop the current
Windows endpoint inventory exposed Realtek speakers and a microphone array;
that is not an eARC capture or twelve-channel physical output path.

**Real Netflix audio remains not tested.** A passing fixture pipeline cannot be
used to mark Netflix compatibility as passing. Netflix says Atmos requires a
compatible plan, playback device, audio system and suitable title/language. On
compatible Windows systems Netflix documents Atmos through Edge or its Windows
app; this does not establish delivery of encoded JOC into Aurora.

Sources checked 2026-09-13:
- [Netflix Atmos requirements](https://help.netflix.com/en/node/64066)
- [Netflix on Windows](https://help.netflix.com/en/node/23931)

An actual Netflix acceptance test remains a separate future physical experiment
using ordinary supported playback and an authorized audio input. No service
stream extraction, decryption, TV identity impersonation or DRM workaround is
part of this harness. A browser playback success alone would not complete that
acceptance test.
