#!/usr/bin/env bash
set -euo pipefail

EARC_DEVICE="${AURORA_EARC_DEVICE:-hw:AuroraEARC,0}"
OUTPUT_DEVICE="${AURORA_OUTPUT_DEVICE:-aurora_tdm}"
DEPS_ROOT="${AURORA_DEPS_ROOT:-$HOME/aurora-deps}"
EXTRACTOR="${AURORA_EXTRACTOR:-./target/release/aurora-iec61937-extract}"
BRIDGE="${AURORA_HARLETTY_BRIDGE:-$DEPS_ROOT/harletty-bridge/target/release/libharletty_bridge.so}"
ORENDER="${AURORA_ORENDER:-$DEPS_ROOT/Omniphony/omniphony-renderer/target/release/orender}"
LAYOUT="${AURORA_SPEAKER_LAYOUT:-$DEPS_ROOT/Omniphony/layouts/7.1.4.yaml}"

for path in "$EXTRACTOR" "$BRIDGE" "$ORENDER" "$LAYOUT"; do
  [[ -e "$path" ]] || { echo "required R0 file missing: $path" >&2; exit 2; }
done

cat >&2 <<EOF
Aurora R0 live path
  eARC input : $EARC_DEVICE
  bridge     : $BRIDGE
  renderer   : $ORENDER
  layout     : $LAYOUT
  output     : $OUTPUT_DEVICE

WARNING: this script does not release amplifier mute. G1-G7 must pass first.
EOF

exec_pipeline() {
  arecord -D "$EARC_DEVICE" -f S32_LE -c 2 -r 192000 -t raw \
    | "$EXTRACTOR" --width s32 --codec eac3 \
    | "$ORENDER" - \
        --bridge-path "$BRIDGE" \
        --enable-vbap \
        --speaker-layout "$LAYOUT" \
        --output-backend pipewire \
        --output-device "$OUTPUT_DEVICE"
}

exec_pipeline
