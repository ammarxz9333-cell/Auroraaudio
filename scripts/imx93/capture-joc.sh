#!/usr/bin/env bash
set -euo pipefail

EARC_DEVICE="${AURORA_EARC_DEVICE:-hw:AuroraEARC,0}"
DURATION="${AURORA_CAPTURE_SECONDS:-15}"
EXTRACTOR="${AURORA_EXTRACTOR:-./target/release/aurora-iec61937-extract}"
DEPS_ROOT="${AURORA_DEPS_ROOT:-$HOME/aurora-deps}"
HARLETTY="${AURORA_HARLETTY_CLI:-$DEPS_ROOT/harletty-bridge/target/release/harletty}"
OUTDIR="${AURORA_CAPTURE_DIR:-output/imx93-r0}"
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
BASE="$OUTDIR/earc-$STAMP"

mkdir -p "$OUTDIR"

[[ -x "$EXTRACTOR" ]] || { echo "extractor not executable: $EXTRACTOR" >&2; exit 2; }
[[ -x "$HARLETTY" ]] || { echo "harletty CLI not executable: $HARLETTY" >&2; exit 2; }

set -o pipefail
arecord -D "$EARC_DEVICE" -f S32_LE -c 2 -r 192000 -d "$DURATION" -t raw \
  | tee "$BASE.s32le" \
  | "$EXTRACTOR" --width s32 --codec eac3 \
  > "$BASE.eac3"

if [[ ! -s "$BASE.eac3" ]]; then
  echo 'No E-AC-3 payload extracted. Confirm TV pass-through mode, eARC link and SAI format.' >&2
  exit 3
fi

"$HARLETTY" --codec eac3 --loglevel warn info "$BASE.eac3" | tee "$BASE.info.txt"
sha256sum "$BASE.s32le" "$BASE.eac3" "$BASE.info.txt" > "$BASE.sha256"

echo "Capture artifact base: $BASE"
echo 'G3 requires the Harletty report to say JOC: yes; EAC3 alone is not Atmos proof.'
