#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
OUT="${AURORA_OUT:-$ROOT/out/s6-aarch64}"
WORK="${AURORA_WORK:-$ROOT/.work/s6-aarch64}"
TREE="$WORK/initramfs"

fail() { echo "build-initramfs-native-aarch64: $*" >&2; exit 1; }
[ "$(uname -m)" = "aarch64" ] || fail "native aarch64 builder required"
[ -f /etc/alpine-release ] || fail "Alpine builder required"

if [ ! -x /bin/busybox.static ]; then
    [ "$(id -u)" -eq 0 ] || fail "busybox-static missing; install it as root with: apk add busybox-static xz"
    apk add --no-cache busybox-static xz
fi
command -v xz >/dev/null 2>&1 || fail "xz missing"

rm -rf "$TREE"
mkdir -p "$TREE/bin" "$TREE/dev" "$TREE/proc" "$TREE/sys" "$TREE/newroot"
install -m 0755 /bin/busybox.static "$TREE/bin/busybox.static"
install -m 0755 "$ROOT/platform/s6/initramfs/init" "$TREE/init"

mkdir -p "$OUT"
(
    cd "$TREE"
    find . -print0 \
      | sort -z \
      | /bin/busybox.static cpio --null -o -H newc \
      | xz --check=crc32 -9e > "$OUT/aurora-s6-initramfs.cpio.xz"
)
sha256sum "$OUT/aurora-s6-initramfs.cpio.xz" > "$OUT/aurora-s6-initramfs.cpio.xz.sha256"

echo "Initramfs built: $OUT/aurora-s6-initramfs.cpio.xz"
