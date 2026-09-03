#!/bin/sh
set -eu

# Assemble the Alpine ARM64 root filesystem used by AuroraOS-S6.
# Run on a native AArch64 Alpine builder as root after build-userspace-native-aarch64.sh.

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
STAGE="${AURORA_OUT:-$ROOT/out/s6-aarch64}"
WORK="${AURORA_WORK:-$ROOT/.work/s6-aarch64}"
FIRMWARE_DIR="${AURORA_STOCK_FIRMWARE:-$ROOT/out/s6-stock-firmware}"
ALPINE_VERSION="${ALPINE_VERSION:-3.24.1}"
ALPINE_SERIES="${ALPINE_SERIES:-v3.24}"
ROOTFS="$WORK/rootfs"
ARCHIVE="$WORK/alpine-minirootfs-${ALPINE_VERSION}-aarch64.tar.gz"
BASE_URL="https://dl-cdn.alpinelinux.org/alpine/${ALPINE_SERIES}/releases/aarch64"

fail() { echo "assemble-rootfs-native-aarch64: $*" >&2; exit 1; }
[ "$(id -u)" -eq 0 ] || fail "run as root"
[ "$(uname -m)" = "aarch64" ] || fail "native aarch64 builder required"
[ -f /etc/alpine-release ] || fail "Alpine Linux builder required"
[ -x "$STAGE/bin/aurora-cli" ] || fail "userspace stage missing; run build-userspace-native-aarch64.sh first"
[ -x "$STAGE/bin/aurora-s6-postprocess" ] || fail "S6 postprocessor stage missing"
[ -x "$STAGE/bin/aurora-ffs-daemon" ] || fail "FunctionFS bridge stage missing"
[ -x "$STAGE/bin/aurora-live-ingest" ] || fail "live streaming ingest stage missing"
[ -x "$STAGE/bin/orender" ] || fail "Omniphony stage missing"
[ -f "$STAGE/lib/libharletty_bridge.so" ] || fail "Harletty bridge stage missing"
[ -x "$STAGE/navidrome/navidrome" ] || fail "Navidrome stage missing"
[ -f "$FIRMWARE_DIR/CAPTURE-MANIFEST.txt" ] || fail "owner-supplied S6 firmware capture missing"

mkdir -p "$WORK"
if [ ! -f "$ARCHIVE" ]; then
    wget -O "$ARCHIVE" "$BASE_URL/$(basename "$ARCHIVE")"
fi
wget -qO "$ARCHIVE.sha256" "$BASE_URL/$(basename "$ARCHIVE").sha256"
(cd "$WORK" && sha256sum -c "$(basename "$ARCHIVE").sha256")

rm -rf "$ROOTFS"
mkdir -p "$ROOTFS"
tar -xzf "$ARCHIVE" -C "$ROOTFS"

# Give the chroot temporary network resolution for package installation.
cp /etc/resolv.conf "$ROOTFS/etc/resolv.conf"
mount -t proc proc "$ROOTFS/proc"
mount -t sysfs sys "$ROOTFS/sys"
mount --bind /dev "$ROOTFS/dev"
cleanup() {
    umount "$ROOTFS/dev" 2>/dev/null || true
    umount "$ROOTFS/sys" 2>/dev/null || true
    umount "$ROOTFS/proc" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

chroot "$ROOTFS" /bin/sh -eu <<'CHROOT'
printf '%s\n' \
  'https://dl-cdn.alpinelinux.org/alpine/v3.24/main' \
  'https://dl-cdn.alpinelinux.org/alpine/v3.24/community' > /etc/apk/repositories
apk update
apk add --no-cache \
  openrc ca-certificates tzdata util-linux e2fsprogs \
  iproute2 iw wpa_supplicant hostapd \
  dbus bluez bluez-openrc \
  alsa-lib pipewire pipewire-tools \
  libgcc libstdc++ libdrm freetype \
  curl jq ffmpeg
rc-update add devfs sysinit || true
rc-update add dmesg sysinit || true
rc-update add mdev sysinit || true
rc-update add hwdrivers sysinit || true
rc-update add modules boot || true
rc-update add sysctl boot || true
rc-update add hostname boot || true
rc-update add bootmisc boot || true
rc-update add networking default || true
rc-update add dbus default || true
CHROOT

# Remove temporary mounts before mutating files below.
cleanup
trap - EXIT INT TERM

# Overlay Aurora configuration and service files.
cp -a "$ROOT/platform/s6/rootfs/." "$ROOTFS/"

install -d "$ROOTFS/usr/local/bin" "$ROOTFS/usr/local/sbin" \
           "$ROOTFS/opt/aurora/external" "$ROOTFS/opt/aurora/navidrome" \
           "$ROOTFS/etc/aurora/layouts" "$ROOTFS/var/lib/aurora" \
           "$ROOTFS/var/log/aurora" "$ROOTFS/srv/music" "$ROOTFS/etc/wifi" \
           "$ROOTFS/etc/runlevels/default"
install -m 0755 "$STAGE/bin/aurora-cli" "$ROOTFS/usr/local/bin/aurora-cli"
install -m 0755 "$STAGE/bin/aurora-s6-postprocess" "$ROOTFS/usr/local/bin/aurora-s6-postprocess"
install -m 0755 "$STAGE/bin/aurora-ffs-daemon" "$ROOTFS/usr/local/sbin/aurora-ffs-daemon"
install -m 0755 "$STAGE/bin/aurora-live-ingest" "$ROOTFS/usr/local/sbin/aurora-live-ingest"
install -m 0755 "$ROOT/platform/s6/rootfs/etc/init.d/aurora-ffs" "$ROOTFS/etc/init.d/aurora-ffs"
install -m 0755 "$ROOT/platform/s6/rootfs/etc/init.d/aurora-live-ingest" "$ROOTFS/etc/init.d/aurora-live-ingest"
install -m 0755 "$STAGE/bin/orender" "$ROOTFS/opt/aurora/external/orender"
install -m 0755 "$STAGE/lib/libharletty_bridge.so" "$ROOTFS/opt/aurora/external/libharletty_bridge.so"
install -m 0644 "$STAGE/share/omniphony/layouts/7.1.4.yaml" "$ROOTFS/etc/aurora/layouts/7.1.4.yaml"
cp -a "$STAGE/navidrome/." "$ROOTFS/opt/aurora/navidrome/"
ln -sf /etc/init.d/aurora-ffs "$ROOTFS/etc/runlevels/default/aurora-ffs"
ln -sf /etc/init.d/aurora-live-ingest "$ROOTFS/etc/runlevels/default/aurora-live-ingest"

# Device-owned Broadcom firmware: keep it outside source control and place it at
# the paths compiled into the zeroflte bcmdhd kernel configuration.
for f in "$FIRMWARE_DIR"/wifi/*; do
    [ -f "$f" ] || continue
    install -m 0644 "$f" "$ROOTFS/etc/wifi/$(basename "$f")"
done
install -d "$ROOTFS/lib/firmware/brcm"
for f in "$FIRMWARE_DIR"/bluetooth/*; do
    [ -f "$f" ] || continue
    install -m 0644 "$f" "$ROOTFS/lib/firmware/brcm/$(basename "$f")"
done

[ -f "$ROOTFS/etc/wifi/bcmdhd_sta.bin" ] || fail "bcmdhd_sta.bin missing from assembled rootfs"
[ -f "$ROOTFS/etc/wifi/nvram_net.txt" ] || echo "WARN: nvram_net.txt not captured under expected name; Wi-Fi bring-up must verify board NVRAM path" >&2

# No Android phone stack, graphical desktop, modem daemon, or browser is installed.
printf 'aurora-s6\n' > "$ROOTFS/etc/hostname"
printf 'AuroraOS-S6 Alpine %s\n' "$ALPINE_VERSION" > "$ROOTFS/etc/aurora-release"
cp "$STAGE/BUILD-MANIFEST.txt" "$ROOTFS/etc/aurora/BUILD-MANIFEST.txt"

# Do not include resolver state from the builder.
: > "$ROOTFS/etc/resolv.conf"

tar --numeric-owner --xattrs --acls -C "$ROOTFS" -czf "$STAGE/AuroraOS-S6-rootfs-${ALPINE_VERSION}-aarch64.tar.gz" .
sha256sum "$STAGE/AuroraOS-S6-rootfs-${ALPINE_VERSION}-aarch64.tar.gz" > "$STAGE/AuroraOS-S6-rootfs-${ALPINE_VERSION}-aarch64.tar.gz.sha256"

echo "Rootfs assembled: $STAGE/AuroraOS-S6-rootfs-${ALPINE_VERSION}-aarch64.tar.gz"