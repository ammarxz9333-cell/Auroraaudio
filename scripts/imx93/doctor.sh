#!/usr/bin/env bash
set -euo pipefail

EARC_DEVICE="${AURORA_EARC_DEVICE:-hw:AuroraEARC,0}"
EXTRACTOR="${AURORA_EXTRACTOR:-./target/release/aurora-iec61937-extract}"

fail=0
check_cmd() {
  if command -v "$1" >/dev/null 2>&1; then
    printf 'OK   command %-12s %s\n' "$1" "$(command -v "$1")"
  else
    printf 'MISS command %s\n' "$1"
    fail=1
  fi
}

for cmd in arecord aplay pw-cli sha256sum timeout; do
  check_cmd "$cmd"
done

arch=$(uname -m)
printf 'INFO architecture: %s\n' "$arch"
if [[ "$arch" != "aarch64" ]]; then
  echo 'WARN R0 target is aarch64/i.MX93; software checks may still run here.'
fi

if [[ -r /proc/cpuinfo ]]; then
  grep -m1 -E 'model name|Processor|CPU part' /proc/cpuinfo || true
fi

if [[ -x "$EXTRACTOR" ]]; then
  echo "OK   extractor $EXTRACTOR"
else
  echo "MISS extractor $EXTRACTOR"
  echo '     build with: cargo build --release -p aurora-audio-io --bin aurora-iec61937-extract'
  fail=1
fi

echo 'INFO ALSA capture devices:'
arecord -l || true

echo "INFO probing $EARC_DEVICE for up to 3 seconds; no eARC clock may cause timeout."
if timeout 3 arecord -D "$EARC_DEVICE" -f S32_LE -c 2 -r 192000 -d 1 -t raw /dev/null >/tmp/aurora-arecord.log 2>&1; then
  echo 'OK   SAI1 capture open/read'
else
  echo 'WARN capture did not complete; inspect /tmp/aurora-arecord.log and verify eARC clock/pinmux.'
fi

exit "$fail"
