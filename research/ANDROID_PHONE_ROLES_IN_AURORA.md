# Android Phone Roles in Aurora

## Conclusion

An Android phone should not be treated as the primary eARC or HDMI multichannel capture device. Commodity phones expose USB-C/HDMI primarily as output paths and do not provide a documented eARC receiver path comparable to Khadas VIM3L or a dedicated eARC receiver IC.

Android can still add substantial value to Aurora in four bounded roles:

1. calibration and measurement node;
2. control and user-interface client;
3. buffered network audio endpoint for non-cinema-critical zones;
4. USB-audio host or device-testing platform.

It must not be placed in the hard realtime cinema synchronization path until device-specific latency, drift, routing, and lifecycle behavior are measured.

## 1. Calibration node

A phone can contribute:

- one or more built-in microphones;
- accelerometer and gyroscope data;
- room-position metadata;
- synchronized test-tone recording over the Aurora control plane;
- coarse room-response, delay, polarity, and level measurements;
- guided speaker-placement workflows.

The built-in phone microphone is not assumed to be flat or absolutely calibrated. Aurora must treat it as a relative measurement sensor unless a device-specific microphone calibration profile is available.

Recommended use:

- relative impulse response;
- speaker arrival-time estimation;
- gross polarity errors;
- left/right and channel identity checks;
- repeated spatial measurements across multiple phone positions.

Not acceptable without calibration:

- absolute SPL certification;
- production-grade frequency-response equalization above the confidence band;
- legal or safety noise measurements.

## 2. Control-plane client

This is the lowest-risk and highest-confidence phone role.

An Aurora Android client can provide:

- room and zone control;
- source selection;
- volume and mute;
- speaker assignment;
- calibration workflow;
- health and latency dashboards;
- device discovery and pairing;
- playback queue and presets;
- diagnostics upload and log retrieval.

The phone remains outside the realtime audio callback and does not become a single point of failure for playback.

## 3. Buffered network audio endpoint

Android supports native low-latency audio through AAudio and the Oboe wrapper. However, achievable latency is device dependent, exclusive mode is not guaranteed, and background scheduling, power management, thermal throttling, routing changes, and OEM audio effects can disturb deterministic playback.

Therefore a phone can be used initially for:

- kitchen or bathroom music;
- temporary speaker endpoint;
- headphone endpoint;
- test receiver;
- portable monitor;
- non-synchronized announcement playback.

It is not initially accepted for:

- wireless rear channels;
- center or LFE channels;
- lip-sync-critical cinema playback;
- phase-coherent multi-speaker rendering.

An Android endpoint must implement:

- Aurora clock estimation;
- jitter buffer;
- sample-rate drift correction;
- monotonic presentation timestamps;
- reconnect and resynchronization;
- xrun and late-packet counters;
- device-specific latency calibration;
- wakelock and foreground-service policy;
- thermal and battery monitoring.

## 4. USB audio host

Android devices can operate in USB host mode when supported by their hardware. Basic Android USB audio support historically guaranteed only a restricted subset, but newer platform work is adding validated multichannel tests.

Android 17 CTS Verifier includes validation for:

- four-channel USB audio input;
- quad, 5.1, and 7.1 USB audio output.

This is strategically useful, but it does not imply that all existing phones support those paths. Every phone and USB interface combination must be capability-probed.

Possible Aurora uses:

- test an XMOS UAC2 bridge;
- attach a class-compliant microphone or measurement interface;
- validate multichannel USB output;
- use the phone as a portable diagnostics station.

## 5. HDMI capture cards attached to Android

A UVC HDMI capture device can be connected to some Android phones and tablets in USB host mode. This does not solve the Aurora eARC requirement because typical UVC capture devices provide HDMI source capture, often with stereo audio, and do not operate as an eARC receiver for a television.

It can still help with:

- source-side HDMI experiments;
- metadata inspection;
- video/audio timing observation;
- capture-card compatibility testing.

## 6. Rooted Android or custom ROM

Root access may expose lower-level ALSA devices or permit custom audio policy and HAL modifications, but this path is device-specific and expensive to maintain.

Root/custom-ROM work is allowed only as an experimental adapter and must not become a product requirement.

Potential advantages:

- direct ALSA access;
- disabled OEM processing;
- fixed sample rate;
- persistent foreground audio service;
- custom USB routing.

Risks:

- bootloader restrictions;
- SafetyNet/Play Integrity failures;
- device-specific kernels;
- vendor binary dependencies;
- security and update burden;
- unpredictable support lifetime.

## 7. Recommended Aurora Android architecture

```text
Aurora Core / VIM3L / Linux Host
        |
        | control + telemetry + timestamped audio
        v
Aurora Android App
        |
        +-- control UI
        +-- calibration recorder
        +-- optional buffered network renderer
        +-- USB-audio diagnostics
```

The Android application should reuse Aurora-owned protocol and timing contracts. It must not duplicate the primary DSP or become authoritative for room state.

## 8. First implementation sequence

### Phase A: control and calibration

- native Android application;
- device discovery;
- room/zone control;
- record synchronized test sweeps;
- upload timestamped recordings;
- relative delay, polarity, and response analysis on Aurora Core.

### Phase B: receiver prototype

- Oboe/AAudio output;
- 48 kHz float or PCM16 path;
- timestamped network packets;
- adaptive jitter buffer;
- xrun and latency telemetry;
- device calibration database.

### Phase C: USB audio laboratory

- enumerate connected USB audio devices;
- report supported formats and channel masks;
- test four-channel input and eight-channel output on Android versions and devices that expose them;
- validate future IT6620/XMOS hardware.

## 9. Acceptance gates

A phone may be promoted to a synchronized Aurora endpoint only after all of the following pass:

- stable 48 kHz playback for 24 hours;
- no channel remapping;
- bounded reconnect time;
- measured drift correction;
- stable latency after screen off;
- stable behavior under Wi-Fi roaming;
- no OEM effect or resampler corruption;
- thermal and battery tests;
- deterministic recovery after app process restart.

## Decision

Add Android to Aurora as a supported client and experimental endpoint platform.

Do not use an Android phone as the solution to the HDMI/eARC capture blocker.

Highest-value immediate role: calibration and control application.

Highest-value future engineering role: portable multichannel USB-audio and network-endpoint test platform.
