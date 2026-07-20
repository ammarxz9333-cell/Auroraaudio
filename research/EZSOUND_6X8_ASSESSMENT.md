# ezsound 6x8 Engineering Assessment for Aurora

**Status:** PARTIALLY VERIFIED — functioning hardware and system use are evidenced by the creator, but final open hardware files, production measurements, and independent validation are not yet public.

**Last reviewed:** 2026-07-20

## 1. Executive decision

The ezsound 6x8 is the strongest currently identified ready-made Raspberry Pi 5 multichannel analog I/O candidate for Aurora's downstream DSP/output prototype path.

It is not an HDMI/eARC receiver and does not solve Dolby, DTS, Atmos, HDCP, EDID, or television audio ingestion.

Current Aurora decision:

- Treat ezsound 6x8 as a serious prototype purchase candidate after confirming price, delivery date, and campaign risk.
- Treat its architecture as a high-value reference for Pi 5 TDM, clocking, isolation, PCM3168A integration, ALSA/ASoC, and CamillaDSP operation.
- Do not base Aurora PCB work on it until the promised schematic and PCB files are actually released and their license is inspected.
- Do not treat creator reports as independent laboratory validation.

## 2. Verified product scope

The project is a Raspberry Pi 5-only multichannel sound card based on the Texas Instruments PCM3168A.

Published scope:

- 6 single-ended analog inputs.
- 8 single-ended analog outputs.
- 96 kHz simultaneous multichannel operation target.
- On-board 24.576 MHz audio oscillator.
- Electrical isolation between Raspberry Pi and the analog/audio domain.
- Separate codec/HAT section and detachable power/input/output section.
- 9–15 V external DC power for the audio domain.
- RCA or 2.54 mm header connectivity.
- Raspberry Pi OS driver support claimed to be included.

Primary project sources:

- https://www.crowdsupply.com/ezsound/ezsound-6x8
- https://www.crowdsupply.com/ezsound/ezsound-6x8/updates/our-campaign-is-live
- https://hackaday.io/project/202637-ezsound-6x8-pi5-multichannel-soundcard

## 3. Evidence that the design works

The creator reports that the proof-of-concept ran all channels simultaneously in loopback mode successfully on 2024-11-20.

The creator then integrated the card into a completed Raspberry Pi 5 digital crossover and amplifier:

```text
Analog sources
    -> ezsound 6x8 ADC
    -> Raspberry Pi 5 I2S/TDM capture
    -> CamillaDSP crossover and gain pipeline
    -> Raspberry Pi 5 I2S/TDM playback
    -> ezsound 6x8 DAC
    -> four stereo Class-D amplifier boards
    -> active three-way loudspeakers + monitor output
```

This is useful implementation evidence because it demonstrates simultaneous capture, DSP, and eight-channel playback in a real enclosure, rather than DAC-only bench playback.

However, this remains creator-provided evidence. No independent long-duration test, Audio Precision report, latency report, or reproducible public test suite was found during this review.

Sources:

- https://hackaday.io/project/202637/logs?sort=oldest
- https://hackaday.io/project/204082-pi5-based-digital-crossover-and-amplifier/details

## 4. Why PCM3168A was selected

The creator reports choosing PCM3168A over AD1934 primarily because:

- TI documentation was more detailed.
- Linux driver support was more complete.
- A single codec provides the required six ADC and eight DAC channels.

TI independently confirms that PCM3168A is an active 24-bit multichannel codec with:

- 6 ADC channels.
- 8 DAC channels.
- ADC sample rates up to 96 kHz.
- DAC sample rates up to 192 kHz.
- I2S, left-justified, right-justified, DSP, and TDM formats.
- Independent ADC and DAC master/slave operation.
- I2C or SPI control.
- Mainline Linux ASoC codec-driver availability.
- Typical DAC SNR of 112 dB and ADC SNR of 107 dB under TI test conditions.

TI source:

- https://www.ti.com/product/PCM3168A

## 5. Clock architecture

This is one of the design's most important reusable lessons.

The sound card generates its own audio clock rather than relying on the Raspberry Pi clock. The published design uses a 24.576 MHz oscillator, corresponding to 256 times 96 kHz.

The PCM3168A has separate ADC and DAC digital audio interfaces, but the Raspberry Pi 5 exposes a shared physical audio-clock path for the relevant configuration. The codec must therefore have one side acting as clock producer and the other as clock consumer; both sides must not drive the same clock net.

The creator documented a Raspberry Pi downstream-kernel workaround named `force-dac-cons` to force the DAC side into clock-consumer mode while the ADC side produces the clock. The underlying ASoC limitation was reported as improved in Linux 6.14, but Raspberry Pi OS adoption lagged behind during development.

Aurora implication:

- The clock topology must be treated as a first-class design constraint, not a late device-tree detail.
- Aurora should prefer a solution that works without a permanent private kernel patch.
- The exact current Raspberry Pi OS implementation must be reverified before purchase or integration because kernel support can change.

Source:

- https://hackaday.io/project/202637/log/239409-exploring-sound-on-linux

## 6. Linux integration

The creator's documented stack uses Linux ASoC and device tree.

Key implementation elements:

- Both Raspberry Pi 5 I2S controller pin configurations are enabled.
- Playback and capture are represented as separate DAI links.
- The PCM3168A is controlled over I2C at address 0x45.
- System clock is declared as 24.576 MHz.
- A Raspberry Pi-specific PCM3168A driver change was developed to handle the ADC/DAC clock-producer conflict on older kernel architecture.
- Crowd Supply currently states that drivers are included with Raspberry Pi OS and that the card is automatically detected on boot.

Risk:

The public campaign statement does not yet provide the exact Raspberry Pi OS image, kernel version, overlay name, upstream commit, or regression matrix. Those must be obtained before Aurora marks the integration as production-stable.

## 7. Electrical and mechanical architecture

The prototype began as a four-layer design. The final architecture separated functions so the codec HAT could become a two-layer board:

```text
Pi 5 HAT section
    - PCM3168A
    - digital isolation
    - clocking
    - control and TDM interfaces

IDC interconnects
    -> differential analog signals

Auxiliary section
    - power conversion and regulation
    - single-ended/differential op-amp stages
    - analog inputs and outputs
    - RCA or header connectors
```

This split is particularly relevant to Aurora because it:

- keeps noisy Pi power and digital circuitry away from analog stages;
- permits flexible enclosure placement;
- avoids forcing many RCA connectors onto the HAT footprint;
- allows differential transport between boards;
- reduces routing pressure on the codec board.

The trade-off is a larger assembly, more connectors, more op-amps, more power rails, and greater BOM/assembly complexity than a simple DAC HAT.

Sources:

- https://hackaday.io/project/202637/log/239313-prototype-a-proof-of-concept
- https://hackaday.io/project/202637-ezsound-6x8-pi5-multichannel-soundcard/log/239590-from-proof-of-concept-to-final-version

## 8. Power and isolation

The card does not use Raspberry Pi power for its analog domain. Published information states:

- external 9–15 V DC input;
- isolated audio circuitry;
- dedicated high-quality regulators;
- separate crystal/audio clock;
- power and analog circuitry physically separated from the Pi HAT section.

The creator's prototype notes describe switching generation of positive and negative rails followed by multiple linear regulators for codec and op-amp supplies. The stated design objective was to balance output swing, thermal load, headroom, and noise rejection.

Aurora implication:

This is a good architectural direction for a premium prototype, but complete schematic review and measured noise spectra are necessary before copying its power approach.

## 9. Open-source and licensing status

As of 2026-07-20:

- The Crowd Supply campaign promises complete source files, including PCB layout, under a permissive license.
- The stated release trigger is when boards are shipped to Crowd Supply for delivery to backers.
- Hackaday currently lists no downloadable project files.
- Therefore schematic, PCB, BOM, fabrication outputs, and final license were not yet available for inspection during this review.

Decision:

**Open-hardware reuse is NOT YET VERIFIED.**

Do not claim that Aurora can copy or fork this design until the actual files and license are published.

## 10. Performance evidence and missing measurements

Published design objective:

- better than approximately -90 dB THD+N for roughly AUD 100 parts cost.

Published component-level TI figures are stronger than that target, but codec datasheet values do not equal complete-board performance.

Not yet found publicly:

- Audio Precision or equivalent test report;
- per-channel THD+N;
- noise floor and idle tones;
- inter-channel crosstalk;
- frequency response;
- output level and clipping point;
- input level and ADC clipping point;
- channel-to-channel gain error;
- round-trip latency;
- xruns under sustained simultaneous 6-in/8-out operation;
- thermal and power-consumption measurements;
- EMI/EMC evidence.

Aurora must request these data or reproduce them before production use.

## 11. Confirmed limitations for Aurora

The ezsound 6x8 does not provide:

- HDMI input;
- eARC or ARC reception;
- HDCP handling;
- EDID negotiation;
- Dolby/DTS/Atmos decoding;
- USB Audio Class capture from a television;
- wireless rear-speaker transport;
- multi-room synchronization by itself;
- power amplification.

It solves only this segment:

```text
Aurora DSP on Pi 5
    <-> 6-channel analog capture
    <-> 8-channel analog playback
```

## 12. Comparison with Audio Injector Octo

Based on currently available evidence, ezsound 6x8 is the stronger new-build candidate for a Pi 5 Aurora prototype because:

- it is designed specifically for Raspberry Pi 5;
- it uses an active TI codec with mainline Linux support;
- it uses one multichannel codec instead of relying on a legacy board stack;
- it explicitly addresses audio-domain isolation and independent clocking;
- the creator has demonstrated a completed CamillaDSP crossover/amplifier system;
- current commercial availability is being pursued through Crowd Supply.

Audio Injector remains useful for historical architecture and open-design study, but ezsound should take priority for a purchase feasibility assessment.

This conclusion may change after objective measurements, final pricing, delivery status, and released design files are available.

## 13. Purchase decision gate

Before Aurora purchases the board, verify:

1. Final price including shipping, VAT, and import charges to Germany.
2. Estimated delivery date and crowdfunding risk.
3. Exact supported Raspberry Pi 5 revision and RAM variants.
4. Exact Raspberry Pi OS release and kernel versions.
5. Whether simultaneous 6-in/8-out at 96 kHz is supported in the shipping image.
6. ALSA device names, channel order, and channel maps.
7. CamillaDSP-tested configuration.
8. Whether the board can run playback-only without unnecessary ADC overhead.
9. Output voltage, input voltage, impedance, and clipping limits.
10. Published measurement report or creator-provided raw measurement files.
11. Whether the promised open-source files have been released.
12. Exact hardware and software licenses.

## 14. Aurora integration proposal

### Prototype architecture

```text
Temporary source path
    -> analog stereo input or Linux-native multichannel source
    -> Raspberry Pi 5
    -> CamillaDSP / Aurora processing
    -> ezsound 6x8
    -> 8-channel power amplification
    -> 5.1.2 or 7.1 speaker test bed
```

### What this unlocks immediately

- real eight-channel Aurora output validation;
- channel-order and routing tests;
- room-correction and calibration tests;
- bass management;
- crossover experiments;
- multi-zone analog output experiments;
- simulation-to-hardware correlation;
- long-duration xrun and thermal testing.

### What remains blocked

- stable HDMI/eARC LPCM ingestion;
- licensed proprietary bitstream decoding;
- wireless rear transport and synchronization.

## 15. Final classification

- **Technical feasibility:** VERIFIED by creator implementation and system demonstration.
- **Independent reliability:** UNVERIFIED.
- **Current Pi 5 Linux compatibility:** PARTIALLY VERIFIED.
- **Open hardware availability:** PROMISED, NOT YET VERIFIED.
- **HDMI/eARC solution:** NO.
- **Aurora prototype fit:** HIGH.
- **Aurora production dependency:** PREMATURE.

## 16. Next bounded research task

Do not repeat a general ezsound search. The next pass must answer only:

1. What is the exact campaign price and Germany landed cost?
2. What delivery schedule and refund/campaign terms apply?
3. What exact kernel/overlay commits provide shipping support?
4. Have schematics, PCB files, BOM, and license been released?
5. Are objective measurements available?
6. Is there any independent user report after boards ship?
