#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
AURORA_INSTALL_ROOT="${AURORA_INSTALL_ROOT:-/opt/aurora}"
AURORA_DEPS_ROOT="${AURORA_DEPS_ROOT:-/opt/aurora-deps}"

mkdir -p "$CONFIG_HOME/systemd/user" "$CONFIG_HOME/aurora"

# The checked-in unit uses /opt/aurora as the appliance default. Generate the
# installed unit so an alternate install root stays coherent.
sed "s#/opt/aurora#$AURORA_INSTALL_ROOT#g" \
  "$REPO_ROOT/platforms/imx93/systemd/aurora-r1.service" \
  >"$CONFIG_HOME/systemd/user/aurora-r1.service"
chmod 0644 "$CONFIG_HOME/systemd/user/aurora-r1.service"

ENV_FILE="$CONFIG_HOME/aurora/r1.env"
if [[ ! -e "$ENV_FILE" ]]; then
  sed \
    -e "s#/opt/aurora-deps#$AURORA_DEPS_ROOT#g" \
    -e "s#/opt/aurora#$AURORA_INSTALL_ROOT#g" \
    "$REPO_ROOT/platforms/imx93/systemd/aurora-r1.env.example" >"$ENV_FILE"
  chmod 0644 "$ENV_FILE"
  echo "installed $ENV_FILE"
else
  echo "kept existing $ENV_FILE"
fi

systemctl --user daemon-reload

cat <<EOF
Aurora R1 direct-ALSA user runtime files installed.

Service: $CONFIG_HOME/systemd/user/aurora-r1.service
Env:     $ENV_FILE

R1 intentionally installs no PipeWire fragment.

Next:
  1. Ensure Aurora exists at $AURORA_INSTALL_ROOT.
  2. Build local binaries:
       cargo build --release -p aurora-audio-io --bin aurora-iec61937-extract --bin aurora-alsa-out
  3. Build pinned Harletty/Omniphony:
       AURORA_DEPS_ROOT=$AURORA_DEPS_ROOT scripts/imx93/build-r0-deps.sh
  4. Verify the ALSA devices named in $ENV_FILE exist and the amplifier boards are hard-muted.
  5. Complete R0 G2-G5 plus direct-ALSA/TDM validation before enabling the service.
  6. Only then:
       systemctl --user enable --now aurora-r1.service

For automatic startup before interactive login on a dedicated appliance account,
an administrator can enable lingering once:
  sudo loginctl enable-linger "$USER"
EOF

# Do not start the audio service automatically. Physical amplifier mute/gain
# safety must be validated before any unattended playback path is enabled.
