#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUNTIME_ROOT="${AURORA_RUNTIME_ROOT:-$HOME/.local/share/aurora-runtime}"
PREFIX="$RUNTIME_ROOT/install"
STATE_DIR="${AURORA_STATE_DIR:-$RUNTIME_ROOT/state}"
DEVICE="${AURORA_EARC_DEVICE:-hw:eARC,0}"
ORENDER="${AURORA_ORENDER:-$PREFIX/bin/orender}"
BRIDGE="${AURORA_HARLETTY_BRIDGE:-$PREFIX/lib/libharletty_bridge.so}"
LAYOUT="${AURORA_SPEAKER_LAYOUT:-$PREFIX/share/aurora/omniphony-11.1.4-aurora.yaml}"
OUTPUT_DEVICE="${AURORA_OUTPUT_DEVICE:-}"

CONVERTER="$ROOT_DIR/validation/physical/aurora_alsa_iec61937_stream.py"
mkdir -p "$STATE_DIR"

for path in "$ORENDER" "$BRIDGE" "$LAYOUT" "$CONVERTER"; do
  [[ -e "$path" ]] || { echo "missing Aurora runtime component: $path" >&2; exit 2; }
done
command -v arecord >/dev/null 2>&1 || { echo "arecord not found (install alsa-utils)" >&2; exit 2; }

OUTPUT_ARGS=(--output-backend pipewire)
if [[ -n "$OUTPUT_DEVICE" ]]; then
  OUTPUT_ARGS+=(--output-device "$OUTPUT_DEVICE")
fi

echo "Aurora Pi5 eARC runtime" >&2
echo "  capture: $DEVICE (S32_LE/2ch carrier @ 192 kHz)" >&2
echo "  bridge : $BRIDGE" >&2
echo "  layout : $LAYOUT" >&2
echo "  output : pipewire ${OUTPUT_DEVICE:-default}" >&2

# Binary-only stdout from the converter feeds orender. Diagnostics stay on
# stderr/status files, so no text can corrupt IEC61937 framing.
python3 "$CONVERTER" capture \
  --device "$DEVICE" \
  --iec-out - \
  --status "$STATE_DIR/earc-status.json" \
  --hw-params-log "$STATE_DIR/earc-hw-params.txt" \
  --stderr-log "$STATE_DIR/arecord.stderr.log" \
  | "$ORENDER" - \
      --bridge-path "$BRIDGE" \
      --enable-vbap \
      --speaker-layout "$LAYOUT" \
      "${OUTPUT_ARGS[@]}"
