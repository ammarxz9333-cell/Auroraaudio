#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
EARC_DEVICE="${AURORA_EARC_DEVICE:-hw:AuroraEARC,0}"
DURATION="${AURORA_CAPTURE_SECONDS:-15}"
EXTRACTOR="${AURORA_EXTRACTOR:-$REPO_ROOT/target/release/aurora-iec61937-extract}"
DEPS_ROOT="${AURORA_DEPS_ROOT:-$HOME/aurora-deps}"
HARLETTY="${AURORA_HARLETTY_CLI:-$DEPS_ROOT/harletty-bridge/target/release/harletty}"
OUTDIR="${AURORA_CAPTURE_DIR:-$REPO_ROOT/output/imx93-r0}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BASE="$OUTDIR/earc-$STAMP"

mkdir -p "$OUTDIR"

[[ -x "$EXTRACTOR" ]] || { echo "extractor not executable: $EXTRACTOR" >&2; exit 2; }
[[ -x "$HARLETTY" ]] || { echo "harletty CLI not executable: $HARLETTY" >&2; exit 2; }
command -v arecord >/dev/null 2>&1 || { echo 'arecord is required' >&2; exit 2; }
command -v sha256sum >/dev/null 2>&1 || { echo 'sha256sum is required' >&2; exit 2; }

arecord -q -D "$EARC_DEVICE" -f S32_LE -c 2 -r 192000 -d "$DURATION" -t raw \
  | tee "$BASE.s32le" \
  | "$EXTRACTOR" --width s32 --codec eac3 \
  >"$BASE.eac3"

if [[ ! -s "$BASE.eac3" ]]; then
  echo 'FAIL G3: no E-AC-3 payload extracted. Confirm TV pass-through, eARC link and SAI format.' >&2
  exit 3
fi

"$HARLETTY" --codec eac3 --loglevel warn info "$BASE.eac3" | tee "$BASE.info.txt"

# This script is specifically the JOC/Atmos capture gate. E-AC-3 alone is not a
# pass: require Harletty's parsed JOC flag before writing the evidence hash set.
if ! grep -Eiq '^[[:space:]]*JOC[[:space:]]*:[[:space:]]*yes([[:space:]]|$)' "$BASE.info.txt"; then
  echo "FAIL G3: extracted stream is E-AC-3 but Harletty did not report JOC: yes" >&2
  echo "Evidence retained at $BASE.* for diagnosis, but the gate is NOT passed." >&2
  exit 4
fi

sha256sum "$BASE.s32le" "$BASE.eac3" "$BASE.info.txt" >"$BASE.sha256"

echo "PASS G3 software check: Harletty reports JOC: yes"
echo "Capture artifact base: $BASE"
