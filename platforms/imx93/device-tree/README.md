# i.MX93 Device-Tree Requirements

## Why this is a requirements file instead of a fake drop-in DTS

The byteENGINE OSM module exposes the required mux functions, but the exact carrier BSP controls node labels, IOMUX macros, clock parents and regulators. A syntactically plausible DTS copied from an i.MX8 board would be more dangerous than useful.

Use the exact byteENGINE/NXP i.MX93 BSP as the base and implement these two cards.

## Card A — `AuroraEARC` capture

Required hardware mux:

```text
AA21 / D21 -> SAI1_RX_BCLK
AA20 / D20 -> SAI1_RX_SYNC
V21  / H20 -> SAI1_RX_DATA00
```

Requirements:

- SAI1 RX is clock consumer; SiI9437 provides BCLK and frame sync;
- no sample-rate/sample-format conversion before IEC61937 extraction;
- expose raw S32_LE capture;
- support 2 channels at nominal 192 kHz for DD+/E-AC-3 IEC61937;
- use direct `hw:` in production capture.

The external-source pattern is not hypothetical: the Vibesbox SiI9437 tap uses a `simple-audio-card` with a `linux,spdif-dir` capture stub and marks the codec/source side as bit-clock and frame master. Its physical capture delivers 24-bit-left-justified words in S32 slots. Apply the same ASoC relationship to the i.MX93 SAI, using the exact BSP labels/pinctrl.

Conceptual skeleton:

```dts
/ {
    aurora_earc_codec: aurora-earc-codec {
        compatible = "linux,spdif-dir";
        #sound-dai-cells = <0>;
    };

    sound-aurora-earc {
        compatible = "simple-audio-card";
        simple-audio-card,name = "AuroraEARC";
        simple-audio-card,format = "i2s";
        simple-audio-card,bitclock-master = <&aurora_earc_dai>;
        simple-audio-card,frame-master = <&aurora_earc_dai>;

        simple-audio-card,cpu {
            sound-dai = <&sai1>;
        };
        aurora_earc_dai: simple-audio-card,codec {
            sound-dai = <&aurora_earc_codec>;
        };
    };
};

&sai1 {
    /* exact byteENGINE i.MX93 pinctrl: RX_BCLK/RX_SYNC/RX_DATA00 */
    status = "okay";
};
```

Do not ship this skeleton unchanged. G2 must archive the actual compiled DTS/DTB source, kernel/BSP revision, ALSA hw params and scope captures.

## Card B — `AuroraTDM16` playback

Hardware mux target:

```text
T3  / R21 -> SAI3_TX_BCLK
M21 / V20 -> SAI3_TX_SYNC
F4  / T21 -> SAI3_TX_DATA00
T4  / R20 -> SAI3_MCLK
```

Target PCM framing:

```text
16 channels
48 kHz
S32_LE
16 x 32-bit slots
BCLK = 24.576 MHz (512fs)
format = dsp_b
```

Upstream Linux/NXP software support is explicit:

- `fsl,imx-audio-card` accepts up to two codec DAIs and `dsp_b`;
- the AK4458 TDM channel constraint includes 16 channels;
- TDM512 is represented as a 512-bit frame and the NXP machine driver selects a 1024fs AK4458 MCLK relationship for that mode (49.152 MHz at 48 kHz);
- the AK4458 codec driver accepts S16/S24/S32 PCM, `DSP_B`, TDM slots, and enables its daisy-chain bit when TDM/DSP_B playback exceeds one chip's eight channels.

That closes the Linux software-model question. G6 still has to prove that the exact i.MX93 BSP can synthesize the required SAI3 clocks on the chosen pins and that the physical two-chip chain has the intended slot order.

Conceptual skeleton:

```dts
/ {
    sound-aurora-tdm16 {
        compatible = "fsl,imx-audio-card";
        model = "AuroraTDM16";

        pri-dai-link {
            link-name = "ak4458-r0";
            format = "dsp_b";
            dai-tdm-slot-num = <16>;
            dai-tdm-slot-width = <32>;
            playback-only;

            cpu {
                sound-dai = <&sai3>;
            };
            codec {
                sound-dai = <&ak4458_1>, <&ak4458_2>;
            };
        };
    };
};

&sai3 {
    /* exact byteENGINE i.MX93 SAI3 TX pinctrl + valid MCLK parent/rates */
    status = "okay";
};
```

The actual carrier DTS must also define both `asahi-kasei,ak4458` I2C codec nodes, their unique strap/I2C addresses, reset/mute GPIOs if used, and all regulator supplies required by the selected analog/digital power tree. Do not invent those values before the carrier schematic is frozen.

## Stable ALSA identifiers

The runtime contract expects:

```text
hw:AuroraEARC,0
hw:AuroraTDM16,0
```

If the final BSP exposes different IDs, update `~/.config/aurora/r0.env` and the PipeWire adapter together. Do not change only one side.

## G6 physical proof

With amplifiers hard-muted, the final DTS must pass:

```text
aplay -D hw:AuroraTDM16,0 -f S32_LE -c 16 -r 48000
```

using a one-slot-at-a-time test stream, with scope confirmation of BCLK/LRCLK/MCLK and measured analog mapping of all sixteen DAC outputs. See `docs/imx93-r0/CHANNEL_MAP.md`.
