#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
OUT="${AURORA_OUT:-$ROOT/out/s6-aarch64}"
WORK="${AURORA_WORK:-$ROOT/.work/s6-aarch64}"
SRC="$WORK/kernel-universal7420"
KERNEL_REPO="https://github.com/samsungexynos7420/android_kernel_samsung_universal7420.git"
KERNEL_COMMIT="${KERNEL_COMMIT:-78bff646ad0fc193f50c4ce357f8b1863786bb98}"
DEFCONFIG="exynos7420-zeroflte_defconfig"
JOBS="${JOBS:-4}"

# This source tree carries modern Clang compatibility backports, but exact
# compiler validation is still part of the hardware bring-up gate. The caller
# may override CC and CROSS_COMPILE explicitly.
CC="${CC:-clang}"
CROSS_COMPILE="${CROSS_COMPILE:-aarch64-linux-gnu-}"

fail() { echo "build-kernel: $*" >&2; exit 1; }
for cmd in git make "$CC"; do command -v "$cmd" >/dev/null 2>&1 || fail "missing tool: $cmd"; done
command -v "${CROSS_COMPILE}as" >/dev/null 2>&1 || fail "missing GNU cross-binutils prefix ${CROSS_COMPILE} (assembler not found)"

mkdir -p "$WORK" "$OUT/kernel"
if [ ! -d "$SRC/.git" ]; then
    git clone "$KERNEL_REPO" "$SRC"
fi
(
    cd "$SRC"
    git fetch origin --tags --force
    git checkout --detach "$KERNEL_COMMIT"
    git reset --hard "$KERNEL_COMMIT"
    git clean -fdx

    export ARCH=arm64
    export CC="$CC"
    export CROSS_COMPILE="$CROSS_COMPILE"

    make "$DEFCONFIG"

    # Preserve the known-good S6 scheduler/timer baseline while replacing only
    # the Android USB gadget with the userspace FunctionFS gadget Aurora needs.
    scripts/config --file .config \
      --enable ARM64 \
      --enable PREEMPT \
      --enable HIGH_RES_TIMERS \
      --enable HZ_250 \
      --disable HZ_1000 \
      --set-val HZ 250 \
      --enable USB \
      --enable USB_DWC3 \
      --enable USB_DWC3_DUAL_ROLE \
      --enable USB_GADGET \
      --disable USB_G_ANDROID \
      --disable USB_ANDROID_SAMSUNG_COMPOSITE \
      --enable USB_LIBCOMPOSITE \
      --enable USB_FUNCTIONFS \
      --enable USB_FUNCTIONFS_GENERIC \
      --enable DEVTMPFS \
      --enable INPUT \
      --enable INPUT_EVDEV \
      --enable FB \
      --enable WLAN

    yes '' | make oldconfig >/dev/null

    # Fail closed if Kconfig silently resolved a required option away.
    require_y() {
        grep -qx "CONFIG_$1=y" .config || {
            echo "Required kernel symbol CONFIG_$1 did not resolve to y" >&2
            exit 1
        }
    }
    require_n() {
        grep -qx "# CONFIG_$1 is not set" .config || {
            echo "Required kernel symbol CONFIG_$1 did not resolve to n" >&2
            exit 1
        }
    }
    for s in ARM64 PREEMPT HIGH_RES_TIMERS USB_DWC3 USB_DWC3_DUAL_ROLE USB_GADGET USB_LIBCOMPOSITE USB_FUNCTIONFS USB_FUNCTIONFS_GENERIC DEVTMPFS INPUT INPUT_EVDEV FB BCM4358 BCMDHD_PCIE; do require_y "$s"; done
    require_n USB_G_ANDROID

    make -j "$JOBS" Image dtbs

    install -m 0644 arch/arm64/boot/Image "$OUT/kernel/Image"
    cp .config "$OUT/kernel/aurora-s6-kernel.config"
    git rev-parse HEAD > "$OUT/kernel/KERNEL-COMMIT.txt"

    rm -rf "$OUT/kernel/dtbs"
    mkdir -p "$OUT/kernel/dtbs"
    find arch/arm64/boot/dts -type f -name '*zeroflte*.dtb' -exec cp '{}' "$OUT/kernel/dtbs/" ';'
    [ "$(find "$OUT/kernel/dtbs" -type f -name '*.dtb' | wc -l)" -gt 0 ] || {
        echo "No zeroflte DTBs were produced" >&2
        exit 1
    }
)

sha256sum "$OUT/kernel/Image" > "$OUT/kernel/Image.sha256"
echo "Kernel staging complete: $OUT/kernel"
echo "Physical S6 boot validation is still required before flash-ready classification."
