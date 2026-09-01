#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
EARC_DEVICE="${AURORA_EARC_DEVICE:-hw:AuroraEARC,0}"
OUTPUT_DEVICE="${AURORA_OUTPUT_DEVICE:-aurora_tdm}"
DEPS_ROOT="${AURORA_DEPS_ROOT:-$HOME/aurora-deps}"
EXTRACTOR="${AURORA_EXTRACTOR:-$REPO_ROOT/target/release/aurora-iec61937-extract}"
BRIDGE="${AURORA_HARLETTY_BRIDGE:-$DEPS_ROOT/harletty-bridge/target/release/libharletty_bridge.so}"
ORENDER="${AURORA_ORENDER:-$DEPS_ROOT/Omniphony/omniphony-renderer/target/release/orender}"
LAYOUT="${AURORA_SPEAKER_LAYOUT:-$REPO_ROOT/platforms/imx93/layouts/aurora-7.1.4.yaml}"

for cmd in arecord pw-dump; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
for path in "$EXTRACTOR" "$BRIDGE" "$ORENDER" "$LAYOUT"; do
  [[ -e "$path" ]] || { echo "missing runtime component: $path" >&2; exit 2; }
done

# Fail closed instead of allowing Omniphony to fall back to a random stereo sink.
# Do not use grep -q here: with pipefail it can close early, SIGPIPE pw-dump and
# turn a successful match into a false-negative pipeline status.
if ! pw-dump 2>/dev/null | grep -F "\"node.name\": \"$OUTPUT_DEVICE\"" >/dev/null; then
  echo "PipeWire sink '$OUTPUT_DEVICE' not found." >&2
  echo 'Install platforms/imx93/pipewire/90-aurora-tdm.conf and restart the user PipeWire service.' >&2
  exit 3
fi

# R0 intentionally accepts the encoded DD+/E-AC-3 path here. The TV must remain
# in eARC pass-through/bitstream mode. The extractor removes IEC61937 while
# preserving the raw E-AC-3 JOC access units. Omniphony reads stdin as Raw and
# Harletty incrementally reassembles/decodes E-AC-3 before VBAP rendering.
exec 3>&2
arecord -q -D "$EARC_DEVICE" -f S32_LE -c 2 -r 192000 -t raw \
  | "$EXTRACTOR" --width s32 --codec eac3 \
  | "$ORENDER" - \
      --bridge-path "$BRIDGE" \
      --enable-vbap \
      --speaker-layout "$LAYOUT" \
      --output-backend pipewire \
      --output-device "$OUTPUT_DEVICE" \
      2>&3

# Amplifier unmute is deliberately outside this script until G7 freezes the
# fail-safe GPIO polarity and analog gain structure. A software crash must not
# be able to release the hardware mute by accident.
