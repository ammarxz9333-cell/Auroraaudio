# Aurora Acoustic Auto-Aim — physical hardware reference (NOT BUILT)

Status: architecture/bench wiring reference only. No actuator, magnetic encoder, amplifier, microphone, chassis or radio transport has been assembled/qualified. The mobile app is LAST.

## Reference single-axis speaker pod

```text
              Aurora core / manual calibration coordinator
                    | verified local/wireless control
                    v
              ESP32-C6 endpoint/control MCU
               |                   |
       MCPWM 50 Hz servo      I2C magnetic encoder
               |                   |
         Separate motor PSU    AS5600 + shaft magnet
               |                   |
              servo ---> rigid speaker pivot / angle feedback
                                    |
                            independent hard end stops

Audio in calibration: Aurora low-volume sweep -> ONE speaker driver
Measurement: independent calibrated 48 kHz USB mic at listener position
Return: bounded mono capture -> deconvolution -> measured response/angle
```

Do not treat a commanded servo pulse as measured rotation. The Espressif servo helper's `read_angle` reports a theoretical value based on the command, **not an encoder reading**. A real magnetic encoder must be mounted on the **moving speaker axis** and mechanically aligned with its shaft/magnet. The AS5600 is a candidate 12-bit absolute rotary position sensor with an I2C interface. This is a design candidate, not a verified integrated board.

## Initial MCU and hardware assumptions

- MCU: ESP32-C6 if reusing Aurora's ESP wireless endpoint evaluation. ESP-IDF MCPWM reference documents servo PWM generation and sample firmware. The exact firmware/IDF pin, radio, I2C and timing compatibility must be verified on the selected board.
- Motor: an RC servo or suitable gearmotor selected **only after** knowing speaker mass, center of gravity, lever arm, rotation resistance and enclosure load. A tiny SG90 should not be assumed sufficient for a finished soundbar pod.
- Position feedback: independent sensor (AS5600 + diametric magnet or mechanically attached angle potentiometer) with plausibility checks, sensor-loss error and settled dwell validation.
- Actuator power: separately rated regulated supply, shared signal-ground arrangement designed for noise/EMI; no servo motor current through ESP32 GPIO or the audio rail. Physical overcurrent/stall protection and hard stops must remain active even if host software hangs.
- Noise: servo movement ONLY during user-requested recalibration while test playback is isolated. Disable motor noise/torque pulses during movie playback if a non-backdrivable mechanical design can securely hold the angle; validate that the chosen mechanism does not slip.
- Keep mechanical motion clear of children/fingers, grille, fan-out cables, up-firing baffle and dock contacts. An unloaded servo test does not qualify the loaded assembly.

## Required MCU/host interface

Host already implements `AIM1|MOVE|epoch|sequence|module-id|angle|crc16` and validates an ACK with measured angle, settled and within-limits flags. See `crates/aurora-measurement/src/auto_aim_motor.rs`.

Actual firmware needs:

1. A hardware watchdog and output power-off/stop path independent of the host.
2. Stale-epoch/sequence and duplicate-replay rejection at the endpoint, not only at the host.
3. Authenticated control origin over Wi-Fi. CRC-16 is only transport corruption detection.
4. A motion timeout, current/stall safety gate, limit switch/encoder plausibility and settled dwell threshold.
5. A measured (not commanded) shaft position in every positive ACK.
6. Emergency stop that does not depend on continued Wi-Fi contact.
7. No motion after configuration is saved unless the user explicitly launches another calibration.

No real firmware implementing all of those gates is in this revision, and no "PHYSICAL-PASS" has been issued.

## Build order before any smartphone UI

A. Host offline calibration and deterministic synthetic test — implemented in this PR.

B. Wire a physical single-axis motor + independent position sensor, choose actual ESP board/pins/firmware, and pass static limits, no-feedback, stall, emergency-stop, unplug/reconnect tests.

C. Connect one USB measurement microphone at the user seat and a known loudspeaker. Record consented 48 kHz unprocessed sweeps; verify end-to-end deconvolution with actual impulse responses and compare the saved vs proposed angle, repeatability and room reflections.

D. Repeat with two speakers, and complete the modular detached audio path with real synchronization and electrical/acoustic evidence. Check amplifier electromagnetic noise and drift.

E. Integrate the same capture protocol into the phone app and add the **manual-only** Recalibrate button. No always-on Wi-Fi sensing/microphone tracking.

## Acceptance criteria for physical tests

- No false SETTLED on position mismatch, disconnected encoder, jam, excessive current or out-of-range movement.
- Power loss/restart starts muted; an old command/epoch cannot restart motion.
- The previously saved preset remains available after an interrupted calibration.
- Every real measurement includes a microphone/clock/device identity, valid SNR/clipping evidence, measured motor position, actual speaker lane, reference sweep identity and repeat trials.
- Optimized direct/early impulse-window score improves compared to the stored angle on repeated measurements, without intolerable tonal/binaural quality loss. These simple scalar energy scores alone cannot prove objective 3D localization, Dolby Atmos or that a specific reflection came from the ceiling.

## Primary hardware reference documentation

- Espressif ESP-IDF MCPWM servo control: https://github.com/espressif/esp-idf/blob/master/examples/peripherals/mcpwm/mcpwm_servo_control/README.md
- ESP32-C6 I2C driver: https://docs.espressif.com/projects/esp-idf/en/latest/esp32c6/api-reference/peripherals/i2c.html
- Espressif servo helper (note its theoretical angle-reporting limitation): https://github.com/espressif/esp-iot-solution/blob/master/docs/en/motor/servo.rst
- ams OSRAM AS5600 magnetic position sensor: https://look.ams-osram.com/m/3ac60b27c91075b5/original/AS5600-FS000133-2-00.pdf

This deliberately does not specify an unverified servo torque, battery rating or ESP GPIO pinout.
