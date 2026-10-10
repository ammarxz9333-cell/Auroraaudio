#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
  echo "build-image.sh requires root for loop-device mounting" >&2
  exit 2
fi
[[ "$#" -eq 2 ]] || {
  echo "usage: $0 BASE_RPI_OS.img OUTPUT_AURORA_OS.img" >&2
  exit 2
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BASE="$1"
OUT="$2"
[[ -f "$BASE" ]] || { echo "base image not found: $BASE" >&2; exit 2; }

cargo build --locked --release -p aurora-os-daemon --manifest-path "$ROOT/Cargo.toml"
cp --reflink=auto "$BASE" "$OUT" 2>/dev/null || cp "$BASE" "$OUT"

LOOP="$(losetup --find --show --partscan "$OUT")"
MNT="$(mktemp -d)"
cleanup() {
  set +e
  mountpoint -q "$MNT" && umount "$MNT"
  losetup -d "$LOOP"
  rmdir "$MNT"
}
trap cleanup EXIT

for _ in {1..20}; do
  [[ -b "${LOOP}p2" ]] && break
  sleep 0.1
done
[[ -b "${LOOP}p2" ]] || { echo "root partition ${LOOP}p2 not found" >&2; exit 1; }

mount "${LOOP}p2" "$MNT"
AURORA_OS_DAEMON_BIN="$ROOT/target/release/aurora-os-daemon" \
  "$ROOT/os/aurora-os/install-overlay.sh" "$MNT"

# Offline image installation cannot use the host account database. Create the
# dedicated service account inside the target rootfs before first boot.
if ! chroot "$MNT" /usr/bin/getent group aurora >/dev/null 2>&1; then
  chroot "$MNT" /usr/sbin/groupadd --system aurora
fi
if ! chroot "$MNT" /usr/bin/id aurora >/dev/null 2>&1; then
  chroot "$MNT" /usr/sbin/useradd --system --gid aurora \
    --home-dir /var/lib/aurora --shell /usr/sbin/nologin aurora
fi
if chroot "$MNT" /usr/bin/getent group audio >/dev/null 2>&1; then
  chroot "$MNT" /usr/sbin/usermod -a -G audio aurora
fi
chroot "$MNT" /bin/mkdir -p /var/lib/aurora /var/cache/aurora
chroot "$MNT" /bin/chown -R aurora:aurora /var/lib/aurora /var/cache/aurora

sync
echo "Aurora OS image written to $OUT"
