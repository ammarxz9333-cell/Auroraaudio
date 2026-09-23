#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONFIG_DIR="$HOME/.config/aurora"
USER_UNIT_DIR="$HOME/.config/systemd/user"
ENV_FILE="$CONFIG_DIR/runtime.env"
UNIT_FILE="$USER_UNIT_DIR/aurora-earc.service"
START_NOW=0

if [[ "${1:-}" == "--start" ]]; then
  START_NOW=1
elif [[ -n "${1:-}" ]]; then
  echo "usage: $0 [--start]" >&2
  exit 2
fi

mkdir -p "$CONFIG_DIR" "$USER_UNIT_DIR"

if [[ ! -f "$ENV_FILE" ]]; then
  cat >"$ENV_FILE" <<EOF
AURORA_REPO_ROOT=$ROOT_DIR
AURORA_OUTPUT_MODE=camilladsp
AURORA_OUTPUT_RATE=48000
AURORA_MASTER_GAIN_DB=-3
AURORA_AUTO_GAIN_CEILING_DB=-1
AURORA_ALSA_OUTPUT_DEVICE=default
AURORA_ALSA_OUTPUT_FORMAT=auto
AURORA_CAMILLADSP_CHUNK=512
AURORA_CAMILLADSP_QUEUELIMIT=2
AURORA_CAMILLADSP_TARGET_LEVEL=512
AURORA_CAMILLADSP_ADJUST_PERIOD=3
# Optional full CamillaDSP room-EQ/FIR config. If unset, Aurora generates
# a flat 16-channel identity config with AsyncSinc clock-drift correction.
# AURORA_CAMILLADSP_CONFIG=
# Optional Omniphony renderer config:
# AURORA_RENDER_CONFIG=
# Diagnostic fallback only:
# AURORA_OUTPUT_MODE=pipewire
# AURORA_OUTPUT_DEVICE=
# AURORA_LATENCY_MS=80
# AURORA_ADAPTIVE_RESAMPLING=1
EOF
  chmod 0600 "$ENV_FILE"
fi

escaped_root="${ROOT_DIR//%/%%}"
cat >"$UNIT_FILE" <<EOF
[Unit]
Description=Aurora Pi5 eARC immersive audio runtime
After=pipewire.service wireplumber.service sound.target
Wants=pipewire.service

[Service]
Type=simple
EnvironmentFile=-%h/.config/aurora/runtime.env
ExecStart=/usr/bin/bash "$escaped_root/scripts/pi5/run-earc-joc.sh"
Restart=on-failure
RestartSec=2
TimeoutStopSec=5

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
systemctl --user enable aurora-earc.service >/dev/null

echo "AURORA-PI5-SERVICE-INSTALL-PASS unit=$UNIT_FILE env=$ENV_FILE"
if [[ "$START_NOW" -eq 1 ]]; then
  systemctl --user restart aurora-earc.service
  systemctl --user --no-pager --full status aurora-earc.service || true
else
  echo "Service enabled for login/startup but not started now."
  echo "Start after eARC + multichannel output are connected:"
  echo "  systemctl --user start aurora-earc.service"
fi
