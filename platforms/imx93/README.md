# i.MX93 R0 Platform Layer

This directory contains the Linux/BSP/runtime integration surface for the selected Aurora R0 compute module.

## Required Linux capabilities

R0 requires two independent ALSA paths:

```text
Capture:  SAI1 RX, externally clocked, S32_LE, 2ch, 192 kHz
Playback: SAI3 TX, clock master, S32_LE, 16ch, 32-bit slots, 48 kHz TDM512
```

The intended stable ALSA IDs are:

```text
hw:AuroraEARC,0
hw:AuroraTDM16,0
```

The exact i.MX93 carrier BSP controls pad macros and clock parents, so the device-tree file remains a requirements/template layer until the real byteENGINE BSP is available.

## Runtime files

- `device-tree/README.md` — SAI1/SAI3 and ASoC requirements.
- `layouts/aurora-7.1.4.yaml` — canonical 12-channel renderer order. Uses PipeWire-compatible `TRL`/`TRR` names while preserving Omniphony's semantic Top-Back labels.
- `pipewire/90-aurora-tdm.conf` — creates the exact `aurora_tdm` 16-channel PipeWire sink backed by `hw:AuroraTDM16,0`.
- `systemd/aurora-r0.service` — **user** service, intentionally in the same PipeWire/WirePlumber session as Omniphony.
- `systemd/aurora-r0.env.example` — appliance runtime paths and device names.

Install the per-user runtime files with:

```bash
./scripts/imx93/deploy-r0-user.sh
```

The deploy script does not enable/start Aurora automatically. G2-G7 must pass first, especially the amplifier mute/gain safety gate.

## Why the playback PCM is 16 channels

Upstream Linux/NXP `fsl,imx-audio-card` has explicit AK4458 TDM support. For TDM it constrains the AK4458 hardware channel count to 1..8 or 16, and TDM512 is explicitly represented as a 512-bit frame with the matching AK4458 MCLK relationship. The AK4458 codec driver accepts DSP_B/TDM, S32_LE and enables daisy-chain mode when the stream exceeds one device's eight channels.

Aurora therefore opens the physical DAC path as 16ch. Omniphony renders the 12 logical 7.1.4 channels into matching PipeWire positions; the four AUX positions remain unused. See `docs/imx93-r0/CHANNEL_MAP.md`.

## Device-tree policy

Do not paste an i.MX8 EVK device tree into an i.MX93 carrier and call it done. Mainline/NXP examples prove the `fsl,imx-audio-card` + AK4458 model and multi-codec/TDM software path, but pad names, clocks and BSP node labels must come from the exact i.MX93 BSP.

## Safety

The software service starts only the decode/render/output path. Amplifier mute/unmute remains a hardware safety interlock until GPIO polarity, startup state and actual KAB9 interface behavior pass G7.
