# i.MX93 Device-Tree Requirements

## Why this is a requirements file instead of a fake drop-in DTS

The byteENGINE OSM module exposes the required mux functions, but the exact carrier BSP controls node labels, IOMUX macros, clock parents and regulators. A syntactically plausible DTS copied from an i.MX8 board would be more dangerous than useful.

Use the current byteENGINE i.MX93 BSP as the base and implement these two cards.

## Card A — `AuroraEARC` capture

Required hardware mux:

```text
AA21 / D21 → SAI1_RX_BCLK
AA20 / D20 → SAI1_RX_SYNC
V21  / H20 → SAI1_RX_DATA00
```

Requirements:

- SAI1 RX uses external BCLK/frame sync supplied by SiI9437;
- no internal sample-rate conversion;
- expose raw S32_LE capture;
- support at least 2 channels at 192 kHz;
- avoid `plughw` in production capture.

A capture-only stub codec such as `linux,spdif-dir` can be used when appropriate for the BSP, because the physical source is a raw serial stream and has no controllable codec on this DAI. Confirm actual ASoC binding behavior on the chosen kernel.

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
        /* CPU is clock consumer; exact master/slave syntax follows BSP binding. */

        simple-audio-card,cpu {
            sound-dai = <&sai1>;
        };
        simple-audio-card,codec {
            sound-dai = <&aurora_earc_codec>;
        };
    };
};

&sai1 {
    /* add byteENGINE v2.1 pinctrl for RX_BCLK/RX_SYNC/RX_DATA00 */
    status = "okay";
};
```

Do not ship this skeleton unchanged. The first acceptance artifact must include the final compiled DTS fragment.

## Card B — `AuroraTDM16` playback

Hardware mux target:

```text
T3  / R21 → SAI3_TX_BCLK
M21 / V20 → SAI3_TX_SYNC
F4  / T21 → SAI3_TX_DATA00
T4  / R20 → SAI3_MCLK
```

Target PCM framing:

```text
16 channels
48 kHz
32-bit slots
dai-tdm-slot-num = 16
dai-tdm-slot-width = 32
```

Linux already contains `fsl,imx-audio-card` handling for AK4458-class codecs and upstream NXP board examples with two AK4458 codec DAIs on one card. Use those as the structural reference, then adapt clocks/pins to i.MX93.

Conceptual skeleton:

```dts
/ {
    sound-aurora-tdm16 {
        compatible = "fsl,imx-audio-card";
        model = "AuroraTDM16";

        pri-dai-link {
            link-name = "ak4458-r0";
            format = "dsp_b"; /* verify exact AK4458/Linux framing during bring-up */
            dai-tdm-slot-num = <16>;
            dai-tdm-slot-width = <32>;

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
    /* add byteENGINE v2.1 SAI3 TX pinctrl and clock parent */
    status = "okay";
};
```

The AK4458 hardware itself must be strapped/configured for TDM512 daisy chain. Linux channel enumeration and the physical daisy-chain slot split must be proven with the G6 test; do not infer channel order from DAI registration alone.

## Bring-up output

The final working BSP integration should result in stable ALSA identifiers, ideally:

```text
hw:AuroraEARC,0
hw:AuroraTDM16,0
```

Scripts accept overrides if the actual ALSA IDs differ.
