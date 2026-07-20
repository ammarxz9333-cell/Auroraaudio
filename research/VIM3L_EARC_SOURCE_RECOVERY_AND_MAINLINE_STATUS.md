# VIM3L eARC source recovery and upstream Linux status

Status: research record only. No runtime or product capability claim.

## Executive conclusion

The Khadas VIM3L remains the only publicly documented board with an end-to-end report of 7.1 LPCM capture from a television over eARC into Linux ALSA. The original experiment used an Amlogic-derived Linux 5.4 tree and the `sm1_s905d3_ac200` device tree, then advertised an eARC capability data structure through the `eARC_RX CDS` ALSA control.

The original public branch was reported as `lukefor/linux` branch `vim3l_earc`. As of 2026-07-20 the repository still exists, but the branch is not discoverable through GitHub branch search and no eARC commits are returned by repository commit search. Treat the original source branch as unavailable until independently recovered from the author, a fork, a local clone, or an archive.

## Verified historical behavior

The Khadas forum thread records:

- successful boot of Amlogic Linux 5.4 on VIM3L using `sm1_s905d3_ac200`;
- USB, SD, eMMC and Ethernet working;
- HDMI display output not required for the use case;
- initial eARC fallback behavior limited to stereo PCM and compressed 5.1;
- successful 7.1 LPCM capture after setting the eARC capability data structure;
- independent confirmation by another user that audio was recorded from a television over eARC.

The reported ALSA command was:

```bash
amixer cset numid=6,iface=MIXER,name='eARC_RX CDS' \
  0x01,0x01,0x08,0x23,0x0f,0x7f,0x07,0x83,0x0f,0x40,0x00,0x00
```

Do not hard-code `numid=6` in Aurora. Resolve the control by name because ALSA control numbering can change with kernel, device tree and card topology.

## Source recovery risk

The forum links to:

```text
https://github.com/lukefor/linux/tree/vim3l_earc
```

Current checks show:

- repository `lukefor/linux` still exists;
- default branch is an old Khadas branch;
- branch search does not return `vim3l_earc`;
- commit search for `earc` returns no results.

Possible explanations:

1. branch deleted;
2. branch renamed;
3. commits became unreachable after force-push;
4. GitHub search indexing limitation;
5. source exists only in a fork or local clone.

Required recovery actions before purchasing hardware solely for this path:

1. search GitHub forks and archive services;
2. contact the original author and independent reproducer;
3. ask Khadas forum participants for a kernel image, config, DTB or local clone;
4. preserve any recovered commit SHA, kernel config, DTB, modules and root filesystem image inside an immutable evidence manifest;
5. verify licensing and preserve attribution before reusing code.

## Mainline Linux status

Mainline support is advancing, but it is not yet equivalent to the vendor eARC implementation.

Verified upstream evidence includes:

- SM1 eARC RX clock definitions merged into the Meson audio clock controller;
- clocks named for the eARC command channel and data channel;
- continued Amlogic audio upstreaming for the S4 family through a v6 patch series in January 2026;
- S4 audio device-tree and clock work covering broad ASoC infrastructure.

This is useful because it reduces future porting risk. It does **not** prove that mainline Linux currently exposes a complete VIM3L eARC ALSA capture device with capability negotiation, link recovery and 7.1 LPCM.

Current classification:

- eARC-related clock infrastructure: VERIFIED upstream;
- generic SM1 audio infrastructure: PARTIAL/VERIFIED by component;
- complete mainline eARC RX driver: NOT VERIFIED;
- VIM3L mainline 7.1 eARC capture: NOT VERIFIED;
- vendor Linux 5.4 VIM3L 7.1 capture: community VERIFIED, not yet reproduced by Aurora.

## Amlogic S4 / AQ222 path

Amlogic continues to upstream audio support for S4 using the AQ222 reference board. This suggests a newer maintainable family may eventually provide a second route.

However:

- AQ222 is a vendor reference board, not a broadly available retail SBC;
- the published series is described as basic audio support;
- no public end-to-end eARC television capture proof was found;
- no retail S4 board with verified eARC RX wiring and ALSA capture was found.

Therefore S4 is a strategic watch item, not a prototype replacement for VIM3L.

## TV-box warning

Mainline Debian/Ubuntu projects support many Amlogic TV boxes, including S905X3 and S905X4 devices. This proves general Linux bootability, not eARC capture capability. Most boxes expose HDMI as an output and may not route the eARC differential pair to the connector or enable the receiver in the device tree.

Do not buy a generic Amlogic box unless all of the following are proven:

- schematic or board trace confirms eARC RX routing;
- eARC command/data channel hardware is present;
- device tree enables the receiver;
- ALSA exposes a capture PCM and `eARC_RX CDS` or equivalent control;
- a multichannel LPCM recording has been demonstrated.

## Reproduction package requirements

A valid Aurora reproduction package must capture:

- board revision and photographs;
- TV model and firmware version;
- HDMI cable and port used;
- bootloader version;
- exact kernel commit and dirty state;
- kernel config;
- DTB source and compiled hash;
- root filesystem image and package manifest;
- `dmesg`, `lsmod`, `aplay -l`, `arecord -l`, `amixer controls`, `amixer contents`;
- exact capability data written to the CDS control;
- exact `arecord` invocation;
- channel-identification WAV and checksum;
- channel order mapping;
- sample-rate and sample-format matrix;
- xrun count and discontinuity log;
- standby, cable disconnect and source-switch recovery evidence;
- 24-hour soak report.

## Engineering decision

1. Keep VIM3L as the highest-priority prototype target.
2. Do not assume the original kernel branch can be rebuilt until source recovery succeeds.
3. In parallel, prepare a clean-room forward-port plan from currently available Amlogic/Khadas source trees.
4. Track mainline Meson eARC-related patches, especially complete RX driver, ASoC card integration and device-tree bindings.
5. Keep IT6620BFN plus XMOS UAC2 as the strongest custom-hardware fallback.
6. Do not accept generic S905D3/S905X3/S905X4 products based only on SoC capability.

## Stop conditions

The VIM3L path must be downgraded if:

- no recoverable source or reproducible image can be obtained;
- eARC capture requires non-redistributable binaries that block the product;
- link recovery is unreliable across representative televisions;
- channel order changes after xrun or reconnect;
- latency cannot satisfy Aurora cinema synchronization targets;
- the board becomes unobtainable before a maintainable replacement is proven.
