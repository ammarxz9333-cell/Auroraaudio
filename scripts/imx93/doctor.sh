#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
EARC_DEVICE="${AURORA_EARC_DEVICE:-hw:AuroraEARC,0}"
TDM_DEVICE="${AURORA_TDM_ALSA_DEVICE:-hw:AuroraTDM16,0}"
OUTPUT_DEVICE="${AURORA_OUTPUT_DEVICE:-aurora_tdm}"
DEPS_ROOT="${AURORA_DEPS_ROOT:-$HOME/aurora-deps}"
EXTRACTOR="${AURORA_EXTRACTOR:-$REPO_ROOT/target/release/aurora-iec61937-extract}"
BRIDGE="${AURORA_HARLETTY_BRIDGE:-$DEPS_ROOT/harletty-bridge/target/release/libharletty_bridge.so}"
ORENDER="${AURORA_ORENDER:-$DEPS_ROOT/Omniphony/omniphony-renderer/target/release/orender}"
LAYOUT="${AURORA_SPEAKER_LAYOUT:-$REPO_ROOT/platforms/imx93/layouts/aurora-7.1.4.yaml}"
HARLETTY_COMMIT=4ccedec804de3b29c02fb2a69575c2f49bf2fb37
OMNIPHONY_COMMIT=44acc87a9cbf4b5ac8f474f51d87851d2c642550

fail=0
check_cmd() {
  if command -v "$1" >/dev/null 2>&1; then
    printf 'OK   command %-12s %s\n' "$1" "$(command -v "$1")"
  else
    printf 'MISS command %s\n' "$1"
    fail=1
  fi
}
check_file() {
  if [[ -e "$1" ]]; then
    printf 'OK   file %s\n' "$1"
  else
    printf 'MISS file %s\n' "$1"
    fail=1
  fi
}

for cmd in arecord aplay pw-dump sha256sum timeout git python3; do
  check_cmd "$cmd"
done

arch="$(uname -m)"
printf 'INFO architecture: %s\n' "$arch"
if [[ "$arch" != "aarch64" ]]; then
  echo 'WARN R0 target is aarch64/i.MX93; integration software can still be checked here.'
fi

check_file "$EXTRACTOR"
check_file "$BRIDGE"
check_file "$ORENDER"
check_file "$LAYOUT"

if [[ -d "$DEPS_ROOT/harletty-bridge/.git" ]]; then
  actual="$(git -C "$DEPS_ROOT/harletty-bridge" rev-parse HEAD 2>/dev/null || true)"
  if [[ "$actual" == "$HARLETTY_COMMIT" ]]; then
    echo "OK   Harletty pin $actual"
  else
    echo "FAIL Harletty pin expected=$HARLETTY_COMMIT actual=$actual"
    fail=1
  fi
fi
if [[ -d "$DEPS_ROOT/Omniphony/.git" ]]; then
  actual="$(git -C "$DEPS_ROOT/Omniphony" rev-parse HEAD 2>/dev/null || true)"
  if [[ "$actual" == "$OMNIPHONY_COMMIT" ]]; then
    echo "OK   Omniphony pin $actual"
  else
    echo "FAIL Omniphony pin expected=$OMNIPHONY_COMMIT actual=$actual"
    fail=1
  fi
fi

if [[ -f "$LAYOUT" ]]; then
  mapfile -t layout_names < <(sed -n 's/^  - name: "\([^"]*\)"/\1/p' "$LAYOUT")
  expected=(FL FR C LFE BL BR SL SR TFL TFR TRL TRR)
  if [[ "${layout_names[*]:-}" == "${expected[*]}" ]]; then
    echo "OK   7.1.4 layout order ${layout_names[*]}"
  else
    echo "FAIL unexpected layout order: ${layout_names[*]:-<none>}"
    fail=1
  fi
fi

echo 'INFO ALSA capture devices:'
arecord -l || true
echo 'INFO ALSA playback devices:'
aplay -l || true

if command -v pw-dump >/dev/null 2>&1; then
  # grep consumes the complete dump; using -q with pipefail can SIGPIPE pw-dump
  # and report a false failure after a valid early match.
  if pw-dump 2>/dev/null | grep -F "\"node.name\": \"$OUTPUT_DEVICE\"" >/dev/null; then
    echo "OK   PipeWire sink $OUTPUT_DEVICE"
  else
    echo "MISS PipeWire sink $OUTPUT_DEVICE"
    echo '     install platforms/imx93/pipewire/90-aurora-tdm.conf and restart PipeWire'
    fail=1
  fi
fi

if command -v systemctl >/dev/null 2>&1; then
  if systemctl --user is-active --quiet pipewire.service 2>/dev/null; then
    echo 'OK   user PipeWire service active'
  else
    echo 'WARN user PipeWire service is not reported active in this session'
  fi
  if systemctl --user is-active --quiet wireplumber.service 2>/dev/null; then
    echo 'OK   user WirePlumber service active'
  else
    echo 'WARN user WirePlumber service is not reported active in this session'
  fi
fi

echo "INFO probing $EARC_DEVICE for up to 3 seconds; no eARC clock may cause timeout."
set +e
timeout 3 arecord -q -D "$EARC_DEVICE" -f S32_LE -c 2 -r 192000 -d 1 -t raw /dev/null >/tmp/aurora-arecord.log 2>&1
capture_status=$?
set -e
if [[ $capture_status -eq 0 ]]; then
  echo 'OK   SAI1 capture open/read at S32_LE 2ch 192k'
else
  echo "WARN capture probe status=$capture_status; inspect /tmp/aurora-arecord.log and verify live eARC clock/pinmux"
fi

# Playback probing writes digital zero only and is opt-in because G7 requires the
# physical amplifier mute state to be known before any output path is opened.
if [[ "${AURORA_PROBE_PLAYBACK:-0}" == "1" ]]; then
  echo "INFO zero-stream probing $TDM_DEVICE as S32_LE 16ch 48k for 1 second"
  set +e
  timeout 1 aplay -q -D "$TDM_DEVICE" -f S32_LE -c 16 -r 48000 -t raw /dev/zero >/tmp/aurora-aplay.log 2>&1
  playback_status=$?
  set -e
  if [[ $playback_status -eq 0 || $playback_status -eq 124 ]]; then
    echo 'OK   TDM16 playback PCM opened and accepted zero stream'
  else
    echo "FAIL TDM16 playback probe status=$playback_status; inspect /tmp/aurora-aplay.log"
    fail=1
  fi
else
  echo 'INFO TDM16 active playback probe skipped; set AURORA_PROBE_PLAYBACK=1 only with amplifiers hard-muted.'
fi

exit "$fail"
