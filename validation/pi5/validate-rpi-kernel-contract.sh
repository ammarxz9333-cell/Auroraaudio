#!/usr/bin/env bash
set -euo pipefail

BASE="https://raw.githubusercontent.com/raspberrypi/linux/rpi-6.18.y"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fetch() {
  curl -fsSL "$BASE/$1" -o "$TMP/$2"
}

fetch arch/arm64/boot/dts/broadcom/rp1.dtsi rp1.dtsi
fetch sound/soc/dwc/dwc-i2s.c dwc-i2s.c
fetch sound/soc/generic/simple-card.c simple-card.c
fetch sound/soc/codecs/spdif_receiver.c spdif_receiver.c
fetch sound/soc/codecs/Makefile codecs.Makefile
fetch arch/arm64/configs/bcm2712_defconfig bcm2712_defconfig

grep -q 'rp1_i2s1: i2s@a4000' "$TMP/rp1.dtsi"
grep -q 'compatible = "snps,designware-i2s"' "$TMP/rp1.dtsi"
grep -q 'RP1_DMA_I2S1_RX' "$TMP/rp1.dtsi"
grep -q 'snps,designware-i2s' "$TMP/dwc-i2s.c"
grep -q 'compatible = "simple-audio-card"' "$TMP/simple-card.c"
grep -q 'compatible = "linux,spdif-dir"' "$TMP/spdif_receiver.c"
grep -q 'snd-soc-spdif-rx-y := spdif_receiver.o' "$TMP/codecs.Makefile"
grep -Eq '^CONFIG_SND_DESIGNWARE_I2S=[my]$' "$TMP/bcm2712_defconfig"
grep -Eq '^CONFIG_SND_SIMPLE_CARD=[my]$' "$TMP/bcm2712_defconfig"

echo "AURORA-PI5-KERNEL-CONTRACT-PASS branch=rpi-6.18.y i2s1=clock-consumer-driver-compatible dma=rx simple-card=yes spdif-dir-source=yes"
