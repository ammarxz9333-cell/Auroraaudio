# Licensing Risks

## Phase 0 Rules

- Do not implement Dolby, DTS, HDMI, HDCP, eARC, ARC, or streaming-service capture behavior.
- Do not depend directly on Cavern, `truehdd`, or other renderer/decoder projects with licensing or patent uncertainty.
- Keep optional third-party renderers behind explicit adapter or process boundaries.
- Record all dependencies in `THIRD_PARTY_LICENSES.md`.

## Known Risk Areas

- Object-audio codec decoding may involve patents, trademarks, certification programs, and incompatible licenses.
- HDMI/eARC/HDCP capture introduces legal and hardware compliance concerns.
- Some spatial-rendering projects may use restrictive licenses or include codec logic that must not enter Aurora core.
- Commercial distribution will require a separate legal review.

## Current Checkpoint Status

The first checkpoint uses only common permissive Rust ecosystem dependencies and implements a simple geometric gain simulator. It does not decode proprietary formats or integrate hardware.

