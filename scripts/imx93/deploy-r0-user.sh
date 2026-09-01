#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
AURORA_INSTALL_ROOT="${AURORA_INSTALL_ROOT:-/opt/aurora}"
AURORA_DEPS_ROOT="${AURORA_DEPS_ROOT:-/opt/aurora-deps}"

mkdir -p \
  "$CONFIG_HOME/pipewire/pipewire.conf.d" \
  "$CONFIG_HOME/systemd/user" \
  "$CONFIG_HOME/aurora"

install -m 0644 \
  "$REPO_ROOT/platforms/imx93/pipewire/90-aurora-tdm.conf" \
  "$CONFIG_HOME/pipewire/pipewire.conf.d/90-aurora-tdm.conf"

# The checked-in unit uses /opt/aurora as the appliance default. Generate the
# installed user unit so an alternate AURORA_INSTALL_ROOT stays coherent.
sed "s#/opt/aurora#$AURORA_INSTALL_ROOT#g" \
  "$REPO_ROOT/platforms/imx93/systemd/aurora-r0.service" \
  >"$CONFIG_HOME/systemd/user/aurora-r0.service"
chmod 0644 "$CONFIG_HOME/systemd/user/aurora-r0.service"

ENV_FILE="$CONFIG_HOME/aurora/r0.env"
if [[ ! -e "$ENV_FILE" ]]; then
  sed \
    -e "s#/opt/aurora-deps#$AURORA_DEPS_ROOT#g" \
    -e "s#/opt/aurora#$AURORA_INSTALL_ROOT#g" \
    "$REPO_ROOT/platforms/imx93/systemd/aurora-r0.env.example" >"$ENV_FILE"
  chmod 0644 "$ENV_FILE"
  echo "installed $ENV_FILE"
else
  echo "kept existing $ENV_FILE"
fi

systemctl --user daemon-reload

# The PipeWire fragment is loaded by the per-user PipeWire daemon. Restarting it
# will interrupt current desktop audio; disable with AURORA_RESTART_PIPEWIRE=0.
if [[ "${AURORA_RESTART_PIPEWIRE:-1}" == "1" ]]; then
  systemctl --user restart pipewire.service
  systemctl --user restart wireplumber.service 2>/dev/null || true
fi

cat <<EOF
Aurora R0 user runtime files installed.

PipeWire: $CONFIG_HOME/pipewire/pipewire.conf.d/90-aurora-tdm.conf
Service:  $CONFIG_HOME/systemd/user/aurora-r0.service
Env:      $ENV_FILE

Next:
  1. Ensure the Aurora repository exists at $AURORA_INSTALL_ROOT (or rerun this installer with the matching path).
  2. Build the Aurora extractor in that checkout.
  3. Build pinned third parties with AURORA_DEPS_ROOT=$AURORA_DEPS_ROOT scripts/imx93/build-r0-deps.sh.
  4. Run scripts/imx93/doctor.sh.
  5. Complete G2-G7 physical gates.
  6. Only then: systemctl --user enable --now aurora-r0.service

For automatic startup before interactive login on a dedicated appliance account,
an administrator can enable lingering once:
  sudo loginctl enable-linger "$USER"
EOF

# Do not start the audio service automatically: G7 requires amplifier mute/gain
# safety to be physically validated first.
