# Head-pose HRTF preparation

`aurora_renderer_basic::binaural::hrtf::DirectionalHrtf` connects the bounded
head-pose timeline to the existing prepared FIR renderer for 1–16 mono object
channels. Input directions must have the same stable order as PCM channels.

On the control thread:

1. Decode the chosen external HRTF data, canonicalize its listener axes, resample
   if needed, and bake delays into bounded left/right FIR coefficients.
2. Prepare a bank with explicit sample rate, tap count and maximum angular error.
   Canonical SOFA Cartesian axes are front/left/up; Aurora axes are right/front/up,
   so the conversion is `(x, y, z) -> (-y, x, z)`.
3. Call `prepare_objects` with mapped pose samples, an explicit logical media frame,
   world-space directions and a new generation. The inverse head orientation gives
   head-local directions. Nearest measured directions select the FIRs. Equal scores
   choose the first entry; duplicate directions and uncovered requests are rejected.
4. Keep the complete candidate alive on the control side. At an exclusive block
   boundary, borrow it into `PreparedBinaural::commit` with an explicit crossfade.
   The callback copies into existing storage and retains convolution history.
   Release candidates on the control side, not inside the callback.

Preparation allocates and is never a callback operation. The processing path and
borrowed commit do not allocate. Rejected preparation cannot mutate a renderer;
the renderer independently rejects incompatible or stale generations and busy
transitions. The caller owns delivery timing, stable channel identity and generation
ordering. A prepared filter snapshot does not automatically expire or schedule itself.

The bank has at most 4096 measurements and 2048 taps per ear. It intentionally uses
explicit nearest-neighbour selection, not HRTF interpolation, distance modelling,
HOA rotation, or personalized HRTF inference. Arbitrary SOFA ListenerView/ListenerUp,
units, receiver ordering and delay conventions must be canonicalized by the external
loader. No SOFA parsing dependency is added to Aurora's runtime.

Validation includes known-axis and yaw-sign assertions, interpolation, object order,
malformed banks, missing/stale poses, angular coverage, transactional rejection and
crossfade PCM. The allocation test prepares pose-selected candidates before measuring
10,000 processing blocks plus commits. The exact-pinned SOFA workflow compares six
MIT KEMAR directions at identity and analytic +90-degree yaw with the external sofar
PCM oracle and retains its corrupted-oracle negative test.

These are software tests. Continuous tracker delivery, runtime scheduling, physical
head tracking, headphone transfer, perceptual quality and physical latency remain
unproven.
