#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
STAGE="${AURORA_OUT:-$ROOT/out/s6-aarch64}"
ALPINE_VERSION="${ALPINE_VERSION:-3.24.1}"
ROOTFS_ARCHIVE="${ROOTFS_ARCHIVE:-$STAGE/AuroraOS-S6-rootfs-${ALPINE_VERSION}-aarch64.tar.gz}"
IMAGE="${SYSTEM_IMAGE:-$STAGE/AuroraOS-S6-system.ext4}"
SIZE_MIB="${SYSTEM_IMAGE_MIB:-1536}"
PARTITION_BYTES=3124019200
WORK="${AURORA_WORK:-$ROOT/.work/s6-aarch64}/system-root"

fail() { echo "make-system-image: $*" >&2; exit 1; }
[ -f "$ROOTFS_ARCHIVE" ] || fail "rootfs archive missing: $ROOTFS_ARCHIVE"
for cmd in mke2fs e2fsck truncate tar sha256sum; do
    command -v "$cmd" >/dev/null 2>&1 || fail "missing tool: $cmd"
done

IMAGE_BYTES=$((SIZE_MIB * 1024 * 1024))
[ "$IMAGE_BYTES" -lt "$PARTITION_BYTES" ] || fail "system image must remain smaller than the 3,124,019,200-byte S6 SYSTEM partition"

rm -rf "$WORK" "$IMAGE"
mkdir -p "$WORK"
tar -xzf "$ROOTFS_ARCHIVE" -C "$WORK"

# mke2fs -d populates the image without loop-mounting it. The resulting ext4
# filesystem is intentionally smaller than the physical SYSTEM partition; this
# keeps build artifacts manageable. It can be grown in-place after validated
# first boot if desired.
truncate -s "$IMAGE_BYTES" "$IMAGE"
mke2fs -q -t ext4 -F -L AURORAOS -m 0 -d "$WORK" "$IMAGE"
e2fsck -fn "$IMAGE"

ACTUAL_BYTES="$(stat -c '%s' "$IMAGE")"
[ "$ACTUAL_BYTES" -le "$PARTITION_BYTES" ] || fail "generated image exceeds physical SYSTEM partition"
sha256sum "$IMAGE" > "$IMAGE.sha256"

cat > "$STAGE/SYSTEM-IMAGE-MANIFEST.txt" <<EOF
format=ext4
label=AURORAOS
image_bytes=$ACTUAL_BYTES
physical_partition_bytes=$PARTITION_BYTES
source_rootfs=$(basename "$ROOTFS_ARCHIVE")
userdata_modified=false
EOF

echo "SYSTEM image built: $IMAGE"
echo "This image is not declared flash-ready until boot/kernel/device validation passes."
