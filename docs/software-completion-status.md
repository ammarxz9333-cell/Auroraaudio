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
