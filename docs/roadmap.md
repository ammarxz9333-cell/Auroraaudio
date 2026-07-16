# Roadmap

## Checkpoint 1: Architecture and Gain Simulation

- Create architecture, roadmap, licensing, and agent guardrail documentation.
- Scaffold the Rust workspace.
- Implement the shared core data model.
- Define the renderer trait.
- Implement one deterministic inverse-distance renderer.
- Add focused unit tests.
- Add a CLI command that prints calculated speaker gains for a moving source.

## Checkpoint 2: Offline WAV Rendering

- Add speaker layout JSON fixtures.
- Add scene trajectory JSON fixtures.
- Add mono WAV input and multichannel WAV output.
- Render a mono source block by block.
- Print peak levels, clipping warnings, and latency metadata.

## Checkpoint 3: Basic DSP

- Implement gain, mute, delay, polarity, high-pass, low-pass, parametric EQ, and simple crossover routing.
- Add channel-independent DSP tests.
- Document future CamillaDSP adapter options in more detail.

## Checkpoint 4: Measurement Skeleton

- Add test-signal generation.
- Add synthetic impulse-response fixtures.
- Implement synthetic time-of-arrival estimation.

## Checkpoint 5: Visualizer

- Build a minimal local UI showing the room, listener, speakers, draggable audio object, gain meters, current distance, delay, and renderer mode.
- Ensure visualizer gains match the offline renderer.

