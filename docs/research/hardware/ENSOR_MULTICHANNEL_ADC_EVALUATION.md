# Ensor Multichannel ADC Evaluation

Status: Candidate (Research)

Purpose:
Evaluate the Ensor multichannel ADC platform as an analog multichannel input backend for Aurora.

Scope:
- Analog multichannel capture (not HDMI/eARC).
- Raspberry Pi 5 I2S/TDM backend.
- DSP development and validation.
- Multi-microphone room measurement.

Pros:
- Up to 8 simultaneous analog channels.
- High sample rates.
- Linux/Raspberry Pi oriented.
- Useful for DSP and calibration development.

Limitations:
- Does not solve HDMI input.
- Does not support HDCP/eARC.
- Prototype maturity must be verified before adoption.

Decision:
Keep as an official research candidate for the Aurora Hardware Compatibility Matrix. Do not adopt as the primary HDMI architecture until independently validated.