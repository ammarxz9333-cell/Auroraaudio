#!/usr/bin/env bash
set -euo pipefail

AURORA_ROOT="${AURORA_INSTALL_ROOT:-/opt/aurora}"
DEPS_ROOT="${AURORA_DEPS_ROOT:-/opt/aurora-deps}"
BIN="${AURORA_APPLIANCE_BIN:-$AURORA_ROOT/target/release/aurora-appliance}"
EARC="${AURORA_EARC_DEVICE:-hw:AuroraEARC,0}"
TDM="${AURORA_TDM_DEVICE:-hw:AuroraTDM16,0}"
LIBORENDER="${AURORA_LIBORENDER:-$DEPS_ROOT/Omniphony/omniphony-renderer/target/release/liborender.so}"
BRIDGE="${AURORA_HARLETTY_BRIDGE:-$DEPS_ROOT/harletty-bridge/target/release/libharletty_bridge.so}"
LAYOUT="${AURORA_SPEAKER_LAYOUT:-$AURORA_ROOT/platforms/imx93/layouts/aurora-7.1.4.yaml}"
QUEUE_MS="${AURORA_QUEUE_MS:-200}"
TARGET_MS="${AURORA_TARGET_MS:-80}"
MAX_PPM="${AURORA_MAX_CLOCK_CORRECTION_PPM:-300}"
GAIN_DB="${AURORA_OUTPUT_GAIN_DB:--3.0}"
LATENCY_FILE="${AURORA_LATENCY_FILE:-/tmp/aurora_delay}"

for path in "$BIN" "$LIBORENDER" "$BRIDGE" "$LAYOUT"; do
  [[ -e "$path" ]] || { echo "missing R2 component: $path" >&2; exit 2; }
done

args=(
  --capture-device "$EARC"
  --playback-device "$TDM"
  --liborender "$LIBORENDER"
  --bridge "$BRIDGE"
  --layout "$LAYOUT"
  --queue-ms "$QUEUE_MS"
  --target-ms "$TARGET_MS"
  --max-ppm "$MAX_PPM"
  --gain-db "$GAIN_DB"
  --latency-file "$LATENCY_FILE"
  --stats
)

if [[ -n "${AURORA_OMNIPHONY_CONFIG:-}" ]]; then
  args+=(--omniphony-config "$AURORA_OMNIPHONY_CONFIG")
fi

exec "$BIN" "${args[@]}"
