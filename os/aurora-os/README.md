# Aurora OS v1

Aurora OS v1 is the appliance layer for the Aurora modular immersive-audio project. It intentionally uses Raspberry Pi OS Lite / Debian arm64 as the kernel, bootloader, networking and package base instead of inventing a new Linux distribution. The Aurora layer owns the audio appliance lifecycle.

## What is integrated

The current mainline stack now contains both:

- detachable speaker-fabric control logic with fail-closed synchronization admission;
- live eARC IEC61937 -> Harletty JOC -> Aurora native VBAP -> adaptive ASRC -> ALSA output.

Aurora OS adds:

- a persistent control daemon;
- systemd boot target and restart policy;
- runtime module inventory at `/run/aurora/modules/*.json`;
- deployment intent at `/var/lib/aurora/intent.json`;
- atomic admitted plan at `/run/aurora/active-plan.json`;
- machine-readable health at `/run/aurora/health.json`;
- `auroractl` for status, health, plan, modules, intents, audio and logs;
- an audio service wrapper around the current live eARC runner;
- a Raspberry Pi OS image injector.

## Boot graph

```text
multi-user.target
      |
  aurora.target
   /    |     \
control audio  health timer
  |      |
speaker   eARC/JOC/VBAP/ASRC
fabric    physical output
```

The control plane always fails closed: if module identity, route capability, clock lock, scheduled playout or skew admission fails, the previous active plan is removed rather than retained.

## Install on a Pi 5

From the repository on a 64-bit Raspberry Pi OS Lite installation:

```bash
sudo os/aurora-os/install-overlay.sh /
sudo systemctl start aurora.target
auroractl health
```

The default config deliberately has `audio.enabled=false`. Configure the actual ALSA capture/output devices in `/etc/aurora/aurora-os.json` only after the selected hardware has passed preflight, then enable audio and restart:

```bash
sudo systemctl restart aurora-audio.service
```

## Build a bootable image

On an arm64 Linux host with loop-device support:

```bash
sudo os/aurora-os/build-image.sh raspberry-pi-os-lite.img aurora-os.img
```

The image builder copies the base image, mounts partition 2, injects Aurora, and enables `aurora.target`. It does not replace the Raspberry Pi kernel or firmware.

## Module state contract

Each endpoint/discovery adapter writes one `SpeakerModuleState` JSON file into `/run/aurora/modules`. Example detached module:

```json
{
  "module_id": "left-pod",
  "capabilities": {
    "output_lanes": 2,
    "dock_pcm": true,
    "wired_network_audio": false,
    "wireless_network_audio": true,
    "battery_powered": true
  },
  "attachment": { "state": "detached" },
  "sync": {
    "locked": true,
    "scheduled_playout": true,
    "estimated_skew_micros": 250,
    "clock_source": "ptp"
  },
  "online": true
}
```

A cinema intent may then assign that module to `surround-left` and `top-rear-left`. The existing modular speaker fabric validates the request and Aurora OS publishes only an admitted plan.

## Truth boundary

Software integration is testable today. The OS does **not** claim physical proof of:

- Pi5 + Lindy/SiI9437 continuous protected streaming-service Atmos;
- UMC1820 + ADA8200 sustained physical 20-slot output;
- ESP wireless sample-accurate detachable-speaker synchronization;
- dock charging/hot-plug electronics;
- a finished battery amplifier PCB;
- Dolby certification.

Those remain hardware acceptance gates. Aurora OS is structured so those drivers/endpoints can be inserted without changing the decoder, renderer or control-plane semantics.
