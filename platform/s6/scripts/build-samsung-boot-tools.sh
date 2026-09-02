#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
OUT="${AURORA_OUT:-$ROOT/out/s6-aarch64}"
WORK="${AURORA_WORK:-$ROOT/.work/s6-aarch64}"
TOOLS="$WORK/samsung-boot-tools"
HW="$WORK/android_hardware_samsung"
DEVICE="$WORK/android_device_samsung_universal7420-common"

HW_REPO="https://github.com/exynos-republic/android_hardware_samsung.git"
HW_COMMIT="${SAMSUNG_BOOT_TOOLS_COMMIT:-e124d2450db9008091a0b0f07f1a8feb1051bbcb}"
DEVICE_REPO="https://github.com/samsungexynos7420/android_device_samsung_universal7420-common.git"
DEVICE_COMMIT="${UNIVERSAL7420_DEVICE_COMMIT:-9794046cc14757576d304f6ebb3e1eaedf1a5845}"
CC="${HOST_CC:-cc}"

fail() { echo "build-samsung-boot-tools: $*" >&2; exit 1; }
for cmd in git "$CC" pkg-config; do command -v "$cmd" >/dev/null 2>&1 || fail "missing tool: $cmd"; done
pkg-config --exists libfdt || fail "libfdt development files missing (Alpine: apk add dtc-dev)"
pkg-config --exists openssl || fail "OpenSSL development files missing (Alpine: apk add openssl-dev)"

mkdir -p "$WORK" "$OUT/tools" "$TOOLS"

if [ ! -d "$HW/.git" ]; then git clone "$HW_REPO" "$HW"; fi
(
  cd "$HW"
  git fetch origin --tags --force
  git checkout --detach "$HW_COMMIT"
  git reset --hard "$HW_COMMIT"
)

if [ ! -d "$DEVICE/.git" ]; then git clone "$DEVICE_REPO" "$DEVICE"; fi
(
  cd "$DEVICE"
  git fetch origin --tags --force
  git checkout --detach "$DEVICE_COMMIT"
  git reset --hard "$DEVICE_COMMIT"
)

SRC="$HW/dtbhtool"
TARGET_INC="$DEVICE/include"
[ -f "$TARGET_INC/samsung_dtbh.h" ] || fail "universal7420 samsung_dtbh.h missing"

# Put the target include path first. The hardware repository carries only a
# deliberately disabled example samsung_dtbh.h; universal7420 supplies the real
# DTBH magic/version/platform/subtype codes used for Galaxy S6 images.
CFLAGS="-O2 -fno-strict-aliasing -I$TARGET_INC -I$SRC/libdtbimg -I$SRC"

"$CC" $CFLAGS \
  "$SRC/mkdtbimg.c" "$SRC/dtbimg.c" \
  $(pkg-config --cflags --libs libfdt) \
  -o "$OUT/tools/dtbhtoolExynos"

"$CC" $CFLAGS \
  "$SRC/mkbootimg.c" "$SRC/dtbimg.c" \
  $(pkg-config --cflags --libs libfdt openssl) \
  -o "$OUT/tools/mkdtbhbootimg"

chmod 0755 "$OUT/tools/dtbhtoolExynos" "$OUT/tools/mkdtbhbootimg"

# Smoke-test the command-line parsers without producing an image.
"$OUT/tools/dtbhtoolExynos" 2>&1 | grep -q 'usage:' || fail "dtbhtoolExynos smoke test failed"
"$OUT/tools/mkdtbhbootimg" 2>&1 | grep -q 'usage:' || fail "mkdtbhbootimg smoke test failed"

{
  echo "hardware_repo=$HW_REPO"
  echo "hardware_commit=$HW_COMMIT"
  echo "device_repo=$DEVICE_REPO"
  echo "device_commit=$DEVICE_COMMIT"
  echo "dtbh_header_sha256=$(sha256sum "$TARGET_INC/samsung_dtbh.h" | awk '{print $1}')"
} > "$OUT/tools/SAMSUNG-BOOT-TOOLS-MANIFEST.txt"

sha256sum "$OUT/tools/dtbhtoolExynos" "$OUT/tools/mkdtbhbootimg" > "$OUT/tools/SHA256SUMS"
echo "Samsung boot tools built: $OUT/tools"
