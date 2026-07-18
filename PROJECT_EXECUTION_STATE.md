# Aurora Project Execution State

## Current state

Aurora is in **active product implementation**.

- Active program: `Immersive Audio Product Implementation 1`
- Active work item: GitHub issue `#43`, Checkpoint A
- Next work item: GitHub issue `#38`
- Default development mode: implementation PRs
- Governance mode: maintenance only

## Source of truth

The implementation sequence, acceptance evidence, and active priorities are defined in:

`docs/roadmaps/immersive-wireless-audio-execution-roadmap.md`

This file is the short entry point for contributors and coding agents. Detailed historical governance documents remain valid records of prior decisions, but they do not override the active implementation roadmap for selecting the next product-development task.

## Required contributor behavior

Contributors and coding agents must:

1. work on one reviewable implementation slice at a time;
2. produce executable code, deterministic tests, and measurable artifacts;
3. run and report the required validation commands;
4. describe experimental and incomplete behavior honestly;
5. avoid adding planning-only layers unless a concrete implementation blocker requires them;
6. avoid combining renderer, HRTF, IAMF, networking, receiver, and multiroom work in one PR.

## Active sequence

1. Correct and validate the landed geometric ITD/ILD binaural prototype under issue `#43`, Checkpoint A.
2. Implement the first offline 3D loudspeaker-rendering vertical slice under issue `#38`.
3. Implement true offline HRTF convolution with licensed or user-supplied HRIR data.
4. Build the deterministic network simulator.
5. Implement packetized IP audio transport.
6. Add Linux and Raspberry Pi-class receiver nodes.
7. Add multiroom product behavior.

## Capability honesty

The current geometric binaural prototype is not a true HRTF renderer and must not be described as Dolby Atmos-like, elevation-capable, or front/back accurate without supporting evidence.

A product capability is complete only when code, tests, artifacts, reproducible commands, and documented limitations exist.