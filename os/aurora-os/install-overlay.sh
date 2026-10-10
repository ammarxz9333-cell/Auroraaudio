#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEST="${1:-/}"
if [[ "$DEST" != "/" ]]; then
  DEST="${DEST%/}"
fi

if [[ "$DEST" == "/" && "${EUID}" -ne 0 ]]; then
  echo "run as root when installing to /" >&2
  exit 2
fi

DAEMON_BIN="${AURORA_OS_DAEMON_BIN:-$ROOT/target/release/aurora-os-daemon}"
if [[ ! -x "$DAEMON_BIN" ]]; then
  cargo build --locked --release -p aurora-os-daemon --manifest-path "$ROOT/Cargo.toml"
fi

install -d "$DEST/usr/local/sbin" "$DEST/usr/local/bin"   "$DEST/usr/lib/systemd/system" "$DEST/etc/aurora" "$DEST/etc/tmpfiles.d"   "$DEST/opt/aurora/src" "$DEST/var/lib/aurora" "$DEST/var/cache/aurora"

install -m 0755 "$DAEMON_BIN" "$DEST/usr/local/sbin/aurora-os-daemon"
install -m 0755 "$ROOT/os/aurora-os/usr/local/sbin/aurora-audio" "$DEST/usr/local/sbin/aurora-audio"
install -m 0755 "$ROOT/os/aurora-os/usr/local/bin/auroractl" "$DEST/usr/local/bin/auroractl"
install -m 0644 "$ROOT/os/aurora-os/usr/lib/systemd/system/"*.service "$DEST/usr/lib/systemd/system/"
install -m 0644 "$ROOT/os/aurora-os/usr/lib/systemd/system/"*.timer "$DEST/usr/lib/systemd/system/"
install -m 0644 "$ROOT/os/aurora-os/usr/lib/systemd/system/"*.target "$DEST/usr/lib/systemd/system/"
install -m 0644 "$ROOT/os/aurora-os/etc/tmpfiles.d/aurora.conf" "$DEST/etc/tmpfiles.d/aurora.conf"
install -m 0644 "$ROOT/os/aurora-os/presets/"*.json "$DEST/usr/share/aurora/presets/"

if [[ ! -e "$DEST/etc/aurora/aurora-os.json" ]]; then
  install -m 0644 "$ROOT/os/aurora-os/etc/aurora/aurora-os.json" "$DEST/etc/aurora/aurora-os.json"
fi

# Install the exact source tree used by the live runner. Keep .git out of the appliance.
tar -C "$ROOT" --exclude=.git --exclude=target --exclude=os/aurora-os -cf - .   | tar -C "$DEST/opt/aurora/src" -xf -

if [[ "$DEST" == "/" ]]; then
  getent group aurora >/dev/null 2>&1 || groupadd --system aurora
  id aurora >/dev/null 2>&1 || useradd --system --gid aurora --home-dir /var/lib/aurora --shell /usr/sbin/nologin aurora
  getent group audio >/dev/null 2>&1 && usermod -a -G audio aurora || true
  chown -R aurora:aurora /var/lib/aurora /var/cache/aurora
  systemd-tmpfiles --create /etc/tmpfiles.d/aurora.conf
  systemctl daemon-reload
  systemctl enable aurora.target
else
  install -d "$DEST/etc/systemd/system/multi-user.target.wants"
  ln -sfn /usr/lib/systemd/system/aurora.target     "$DEST/etc/systemd/system/multi-user.target.wants/aurora.target"
fi

echo "Aurora OS overlay installed into $DEST"
