#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${AURORA_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
RUNTIME_ROOT="${AURORA_RUNTIME_ROOT:-$HOME/.local/share/aurora-runtime}"
PREFIX="$RUNTIME_ROOT/install"
STATE_DIR="${AURORA_STATE_DIR:-$RUNTIME_ROOT/state}"
DEVICE="${AURORA_EARC_DEVICE:-hw:eARC,0}"

fail=0
check() {
  local label="$1"; shift
  if "$@" >/dev/null 2>&1; then
    printf 'PASS  %s\n' "$label"
  else
    printf 'FAIL  %s\n' "$label"
    fail=1
  fi
}

check "orender installed" test -x "$PREFIX/bin/orender"
check "Harletty bridge installed" test -s "$PREFIX/lib/libharletty_bridge.so"
check "Aurora 11.1.4 layout installed" test -s "$PREFIX/share/aurora/omniphony-11.1.4-aurora.yaml"
check "eARC converter present" test -s "$ROOT_DIR/validation/physical/aurora_alsa_iec61937_stream.py"
check "arecord available" command -v arecord
check "configured eARC ALSA endpoint usable" arecord -D "$DEVICE" --dump-hw-params
check "PipeWire client available" bash -c 'command -v wpctl >/dev/null || command -v pw-cli >/dev/null'

if [[ -f "$STATE_DIR/earc-status.json" ]]; then
  python3 - "$STATE_DIR/earc-status.json" <<'PY'
import json, pathlib, sys
p=pathlib.Path(sys.argv[1])
try:
    d=json.loads(p.read_text(encoding="utf-8"))
except Exception as exc:
    print(f"WARN  status JSON unreadable: {exc}")
else:
    print(
        "INFO  ingress "
        f"bytes={d.get('canonical_bytes','?')} "
        f"lane={d.get('selected_word_lane','?')} "
        f"order={d.get('selected_channel_order','?')}"
    )
PY
else
  echo "INFO  no ingress status yet (runtime has not produced one)"
fi

if command -v vcgencmd >/dev/null 2>&1; then
  temp="$(vcgencmd measure_temp 2>/dev/null || true)"
  throttle="$(vcgencmd get_throttled 2>/dev/null || true)"
  [[ -n "$temp" ]] && echo "INFO  Pi $temp"
  [[ -n "$throttle" ]] && echo "INFO  Pi $throttle"
fi

if [[ "$fail" -ne 0 ]]; then
  echo "AURORA-PI5-HEALTH-FAIL device=$DEVICE"
  exit 1
fi

echo "AURORA-PI5-HEALTH-PASS device=$DEVICE"
