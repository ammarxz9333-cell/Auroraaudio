# ESP32-P4 exact SDK compatibility

PR #206 originally selected stock `espressif/idf:v6.0.2`. The full build at
`cdb1ed57aacf38e1d63c46523206ed4619a1880c` failed in
[run 35137941169](https://github.com/ammarxz9333-cell/Auroraaudio/actions/runs/35137941169):
`esp_ptp/ptp.c` cannot include `sys/timex.h`; `ptp_wifi.c` also encounters an
incomplete `wifi_sta_list_t` and a missing FTM report `ppm` field.

This is an SDK incompatibility, not a missing include path. Stock 6.0.2 has
neither `sys/timex.h` nor `esp_eth_clock.h`. The unchanged ESP-PTP source uses
`clock_adjtime`, `CLOCK_PTP_SYSTEM` and `esp_eth_clock_init` for its hardware
clock. The SDK added this interface in commit
`ed2e6735ff907eaacc1d8a254613ec650e949774` (POSIX clock refactoring).
Substituting a software clock or dummy implementations would invalidate the
wired hardware-clock build boundary.

The build reference therefore explicitly changes the SDK to Scramble Tools
ESP-IDF `eff8fd1d0b182429b1b574cba4ae8e9be7afa457` (`6.1.0-dev`), including its
recorded submodule revisions. This upstream fork includes the hardware-clock
API and EMAC fixes used by the endpoint family. The four endpoint/component
pins are unchanged. The `v6.0.2` container is only a bootstrap environment:
the workflow installs the pinned SDK's own tools and exports that SDK before
building. A successful result must **not** be described as stock 6.0.2
compatibility. Source pin checks and the full firmware link remain mandatory.

The Aurora servo-stability/GM instrumentation is unchanged. Build success is
software/toolchain compatibility only, never physical gPTP convergence, AAF
delivery, AVDECC interoperability, synchronization, RF, latency or acoustics.

The separate exact-source medium patch compiles Wi-Fi transport/event code only
when a Wi-Fi PTP port is configured. The wired initializer explicitly returns
`ERROR` for Wi-Fi requests; no success stubs or clock fallbacks are introduced.
It validates HEAD and both pristine source files before writing any changes,
and runs before the unchanged clock-evidence patch. This does not claim a
working C6 or coprocessor Wi-Fi build.
