#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DTS="$ROOT_DIR/platform/pi5/aurora-earc-tap-overlay.dts"
BUILD_DIR="${AURORA_RUNTIME_ROOT:-$HOME/.local/share/aurora-runtime}/overlay"
DTBO="$BUILD_DIR/aurora-earc-tap.dtbo"

command -v dtc >/dev/null 2>&1 || {
  echo "dtc not found; install device-tree-compiler" >&2
  exit 2
}
[[ -f "$DTS" ]] || { echo "missing overlay source: $DTS" >&2; exit 2; }

mkdir -p "$BUILD_DIR"
dtc -@ -I dts -O dtb -o "$DTBO" "$DTS"

BOOT_ROOT="/boot/firmware"
if [[ ! -d "$BOOT_ROOT/overlays" ]]; then
  BOOT_ROOT="/boot"
fi
[[ -d "$BOOT_ROOT/overlays" ]] || {
  echo "Raspberry Pi overlays directory not found" >&2
  exit 2
}
CONFIG="$BOOT_ROOT/config.txt"
[[ -f "$CONFIG" ]] || { echo "Raspberry Pi config not found: $CONFIG" >&2; exit 2; }

sudo install -m 0644 "$DTBO" "$BOOT_ROOT/overlays/aurora-earc-tap.dtbo"
if ! grep -Eq '^[[:space:]]*dtoverlay=aurora-earc-tap([[:space:]]|$)' "$CONFIG"; then
  printf '%s\n' 'dtoverlay=aurora-earc-tap' | sudo tee -a "$CONFIG" >/dev/null
fi

echo "AURORA-EARC-OVERLAY-INSTALL-PASS dtbo=$BOOT_ROOT/overlays/aurora-earc-tap.dtbo"
echo "Reboot once, then verify: arecord -l | grep -i eARC" >&2
