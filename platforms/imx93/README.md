# i.MX93 R0 Platform Layer

This directory contains the Linux/BSP integration surface for the selected Aurora R0 compute module.

## Required Linux capabilities

R0 requires two independent ALSA paths:

```text
Capture:  SAI1 RX, externally clocked, S32_LE, 2ch, 192 kHz
Playback: SAI3 TX, clock master, 16ch, 32-bit slots, 48 kHz TDM512
```

The final node names depend on the byteENGINE BSP. Scripts therefore use environment variables rather than hard-coded ALSA card indexes.

## Device-tree policy

Do not paste an i.MX8 EVK device tree into an i.MX93 carrier and call it done. Mainline/NXP examples prove the `fsl,imx-audio-card` + AK4458 model and multi-codec concept, but pad names, clocks and BSP node labels must come from the exact i.MX93 BSP.

`device-tree/README.md` contains the exact electrical/mux requirements and a reference skeleton. It is intentionally a template until the byteENGINE carrier BSP is in hand.

## Runtime

The systemd unit here starts only the software audio pipeline. Amplifier mute/unmute remains a hardware safety interlock until the GPIO polarity and actual KAB9 interface have been measured.
