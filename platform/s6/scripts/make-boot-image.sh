#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
OUT="${AURORA_OUT:-$ROOT/out/s6-aarch64}"
TOOL="$OUT/tools/mkdtbhbootimg"
KERNEL="$OUT/kernel/Image"
RAMDISK="$OUT/aurora-s6-initramfs.cpio.xz"
DTIMG="$OUT/kernel/aurora-s6-dt.img"
BOOTIMG="$OUT/aurora-s6-boot.img"

# Values are taken from universal7420 BoardConfigCommon.mk.
BOARD="universal7420"
BASE=0x10000000
PAGE_SIZE=2048
KERNEL_OFFSET=0x00008000
RAMDISK_OFFSET=0x01000000
TAGS_OFFSET=0x00000100
BOOT_PARTITION_SIZE=29360128

fail() { echo "make-boot-image: $*" >&2; exit 1; }
for f in "$TOOL" "$KERNEL" "$RAMDISK" "$DTIMG"; do [ -e "$f" ] || fail "missing prerequisite: $f"; done
[ -x "$TOOL" ] || fail "mkdtbhbootimg is not executable"

rm -f "$BOOTIMG"
"$TOOL" \
  --kernel "$KERNEL" \
  --ramdisk "$RAMDISK" \
  --dt "$DTIMG" \
  --board "$BOARD" \
  --base "$BASE" \
  --pagesize "$PAGE_SIZE" \
  --kernel_offset "$KERNEL_OFFSET" \
  --ramdisk_offset "$RAMDISK_OFFSET" \
  --tags_offset "$TAGS_OFFSET" \
  --cmdline "" \
  --output "$BOOTIMG"

[ -s "$BOOTIMG" ] || fail "empty boot image produced"
MAGIC="$(dd if="$BOOTIMG" bs=1 count=8 2>/dev/null || true)"
[ "$MAGIC" = "ANDROID!" ] || fail "unexpected boot image magic: ${MAGIC:-none}"
SIZE="$(wc -c < "$BOOTIMG" | tr -d ' ')"
[ "$SIZE" -le "$BOOT_PARTITION_SIZE" ] || fail "boot image $SIZE exceeds BOOT partition $BOOT_PARTITION_SIZE"

sha256sum "$BOOTIMG" > "$BOOTIMG.sha256"
{
  echo "format=android-bootimg-v0+samsung-dtbh"
  echo "board=$BOARD"
  echo "base=$BASE"
  echo "page_size=$PAGE_SIZE"
  echo "kernel_offset=$KERNEL_OFFSET"
  echo "ramdisk_offset=$RAMDISK_OFFSET"
  echo "tags_offset=$TAGS_OFFSET"
  echo "partition_limit=$BOOT_PARTITION_SIZE"
  echo "image_size=$SIZE"
  echo "kernel_sha256=$(sha256sum "$KERNEL" | awk '{print $1}')"
  echo "ramdisk_sha256=$(sha256sum "$RAMDISK" | awk '{print $1}')"
  echo "dtimg_sha256=$(sha256sum "$DTIMG" | awk '{print $1}')"
} > "$OUT/BOOT-IMAGE-MANIFEST.txt"

echo "Boot image built: $BOOTIMG ($SIZE bytes)"
echo "Status: build-complete, NOT flash-ready until physical SM-G920F boot validation passes."
