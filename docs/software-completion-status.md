# Software completion status

The software-completion implementation gate is closed for the capability surface carried by PR #159.

Validated on the final code head before documentation reconciliation:

- current `main-v2` including OAR stereo and 5.1 object-render differential validation;
- adaptive clock-rate estimator/controller/ASRC evidence;
- bounded reconnect/recovery evidence;
- panic-isolated decoder and immersive runtime recovery;
- live JOC software-reference validation, including moving-object evidence;
- IAMF rendered-channel-PCM reference validation;
- fail-closed live IEC61937 ingress diagnostics;
- sustained realtime health evidence;
- Linux and native-Windows full-system virtual-hardware simulation;
- runtime-placeholder regression gate;
- stable Linux/Windows CI plus Rust 1.78 MSRV checks and tests.

Incomplete runtime adapters were removed or explicitly de-scoped rather than represented as supported implementations. TrueHD remains unsupported as a production runtime adapter; IAMF object-scene decoding remains unavailable until a reviewed backend exposes complete source-object metadata and object-to-PCM bindings.

Software completion does not close physical or external gates. Physical eARC capture, USB/TDM electrical timing, DAC/amplifier loopback, acoustic validation, protected-service compatibility through legitimate platform paths, and certification remain separate evidence requirements.

The canonical post-merge plan is `docs/pre-hardware-roadmap-v5.md`.


## Pi 5 eARC runtime integration update — 2026-09-22

The optional Pi 5 home-theater profile is now assembled as one software path rather than a collection of research candidates. It pins Harletty 0.8.0 with Omniphony 0.6.0, feeds Aurora's canonical IEC61937 stream directly to `orender` stdin, adds the Pi5 RP1 I2S-slave overlay, and carries an Aurora-owned custom 11.1.4 / 16-output layout. The realtime JOC soak targets that 16-output layout on native ARM64 CI.

This closes the **software integration shape** for the selected Pi5 profile. It does not close physical validation: Samsung-TV protected-app playback through the actual Lindy tap, final Pi5 thermal headroom, DAC/amp output, xruns, end-to-end lip-sync and acoustic calibration still require the physical unit.
