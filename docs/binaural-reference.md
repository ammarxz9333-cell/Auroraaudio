# Phase 11 binaural reference baseline

Aurora Pre-Hardware Roadmap Phase 11 starts with two independent external binaural rendering oracles and one Rust-native SOFA/HRTF implementation candidate. This lane is deliberately a provenance/build/test baseline; it does not add a binaural renderer to Aurora runtime.

## Google Open Binaural Renderer (OBR)

Aurora pins `google/obr` at commit `478dc7c752d5eccae534635139ff0253eee3a14a` and keeps it behind an external validation boundary.

The selected upstream tests are semantically useful rather than build-only:

- `obr_impl_test` exercises binaural rendering and Ambisonic directional behavior;
- `ambisonic_rotator_test` exercises third-order soundfield rotations around the x/y/z axes, including interpolation across buffer sizes;
- `obr_cli_lib_test` renders upstream fixtures as 3OA, channel-based 7.1.4, and object-mono input, and also rejects mismatched type/input combinations.

OBR source is distributed under its BSD-style source license together with the separate **Open Binaural Renderer Patent License 1.0**. The patent terms include conditions and defensive termination, so Aurora does not treat a green CI lane as blanket product/legal clearance. OBR remains an external oracle unless a separate adoption and distribution review is made.

## EBU BEAR

Aurora pins `ebu/bear` at commit `6127e897b941211051c2ad135ee09b00be2e6ae0` and treats it as an independent ADM-oriented binaural oracle.

Upstream explicitly describes BEAR as a **pre-release**. Aurora therefore makes no production-readiness claim. The pinned `flake.lock` is part of the evidence boundary and fixes the external EAR, libear, VISR, nixpkgs, and flake-utils revisions consumed by the Nix build.

CI follows the upstream-supported Nix path with `nix build .#bear --no-link`. At the pinned revision, the `visr_bear` derivation enables `BEAR_UNIT_TESTS=true` and `doCheck=true`; its test set includes renderer, panner, variable-block-size, spherical-harmonic rotation, and dynamic-renderer tests. The Python `bear` derivation also runs its test phase when `visr_bear` is present.

BEAR is Apache-2.0, but a source-code license is not evidence of perceptual quality, standards certification, or physical head-tracking behavior.

## `sofar`

Aurora pins `andreiltd/sofar` at commit `06a629292689e99841e5dacaa25c4c6298616ca6`, observed crate version `0.3.0`. It is licensed `MIT OR Apache-2.0` and is evaluated only as a Rust-native candidate for SOFA loading, HRTF handling, and partitioned convolution.

The exact gitlink to `libmysofa` is also part of the contract: `da9e4adc619ee3d1ae5e68da3ed14aa5e60b3ec1`.

The pinned source does not publish a root `Cargo.lock`. CI therefore generates a lock from the exact pinned manifests, records its SHA-256 in Aurora evidence, and then runs `cargo test --locked --workspace --all-features`. That generated lock is an auditable closure for the CI run; it is not represented as an upstream-published dependency pin.

A green result does **not** select `sofar` as Aurora's runtime implementation.

## What this baseline proves

A green `Binaural Reference CI` proves only that Aurora can reproduce the declared external source identities and dependency boundaries, observe the expected licensing/maturity surfaces, and execute the selected upstream build/test gates.

It does not yet establish agreement between independent binaural renderers. The next Phase 11 evidence must use deterministic fixtures and compare bounded semantics rather than byte-identical PCM. Planned dimensions include:

- channel-based 7.1.4 to binaural;
- object and Ambisonics input;
- head-rotation semantics;
- HRTF transition continuity and finite-output safety;
- front/back discrimination;
- elevation discrimination.

Perceptual listening quality, individualized HRTFs, physical tracker accuracy, headphone/device transfer functions, measured end-to-end latency, protected-service behavior, and certification remain outside this software-reference baseline.
