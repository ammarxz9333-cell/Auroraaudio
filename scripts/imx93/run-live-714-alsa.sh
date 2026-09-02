#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
EARC_DEVICE="${AURORA_EARC_DEVICE:-hw:AuroraEARC,0}"
TDM_DEVICE="${AURORA_TDM_DEVICE:-hw:AuroraTDM16,0}"
DEPS_ROOT="${AURORA_DEPS_ROOT:-$HOME/aurora-deps}"
EXTRACTOR="${AURORA_EXTRACTOR:-$REPO_ROOT/target/release/aurora-iec61937-extract}"
ALSA_OUT="${AURORA_ALSA_OUT:-$REPO_ROOT/target/release/aurora-alsa-out}"
BRIDGE="${AURORA_HARLETTY_BRIDGE:-$DEPS_ROOT/harletty-bridge/target/release/libharletty_bridge.so}"
ORENDER="${AURORA_ORENDER:-$DEPS_ROOT/Omniphony/omniphony-renderer/target/release/orender}"
LAYOUT="${AURORA_SPEAKER_LAYOUT:-$REPO_ROOT/platforms/imx93/layouts/aurora-7.1.4.yaml}"
GAIN_DB="${AURORA_OUTPUT_GAIN_DB:--3.0}"
MAX_PPM="${AURORA_MAX_CLOCK_CORRECTION_PPM:-300}"
QUEUE_MS="${AURORA_ALSA_QUEUE_MS:-200}"
TARGET_MS="${AURORA_ALSA_TARGET_MS:-80}"

for cmd in arecord; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
for path in "$EXTRACTOR" "$ALSA_OUT" "$BRIDGE" "$ORENDER" "$LAYOUT"; do
  [[ -e "$path" ]] || { echo "missing runtime component: $path" >&2; exit 2; }
done

# R1 deliberately bypasses PipeWire. The eARC capture device remains the source
# clock. Omniphony writes 12-channel raw f32 at 48 kHz; aurora-alsa-out owns the
# bounded queue, source-vs-DAC clock matching, 12->16 deterministic slot pack,
# f32->S32_LE conversion and direct ALSA writes to the TDM device.
#
# This script must remain fail-closed: if capture, IEC deframing, Harletty,
# Omniphony or the ALSA bridge exits, `pipefail` terminates the appliance
# service rather than silently selecting another audio device.
arecord -q -D "$EARC_DEVICE" -f S32_LE -c 2 -r 192000 -t raw \
  | "$EXTRACTOR" --width s32 --codec eac3 \
  | "$ORENDER" - \
      --bridge-path "$BRIDGE" \
      --enable-vbap \
      --speaker-layout "$LAYOUT" \
      --output-backend file \
      --output-file - \
      --output-file-format raw-f32 \
      --output-sample-rate 48000 \
  | "$ALSA_OUT" \
      --device "$TDM_DEVICE" \
      --queue-ms "$QUEUE_MS" \
      --target-ms "$TARGET_MS" \
      --max-ppm "$MAX_PPM" \
      --gain-db "$GAIN_DB" \
      --stats
