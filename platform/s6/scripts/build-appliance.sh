#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
SCRIPTS="$ROOT/platform/s6/scripts"
OUT="${AURORA_OUT:-$ROOT/out/s6-aarch64}"
PHASE="${1:-all}"

fail() { echo "build-appliance: $*" >&2; exit 1; }
run() { echo ">>> $*"; "$@"; }

native_gate() {
    [ "$(uname -m)" = "aarch64" ] || fail "phase '$PHASE' requires a native AArch64 builder"
    [ -f /etc/alpine-release ] || fail "phase '$PHASE' requires Alpine Linux"
}

build_userspace_phase() {
    native_gate
    [ -f "${AURORA_STOCK_FIRMWARE:-$ROOT/out/s6-stock-firmware}/CAPTURE-MANIFEST.txt" ] || \
        fail "owner S6 Wi-Fi/BT firmware capture missing; run capture-stock-firmware.sh on the owner's device/stock image first"

    run "$SCRIPTS/build-userspace-native-aarch64.sh"
    run "$SCRIPTS/assemble-rootfs-native-aarch64.sh"
    run "$SCRIPTS/make-system-image.sh"
    run "$SCRIPTS/build-initramfs-native-aarch64.sh"
}

build_boot_phase() {
    # Kernel build supports either a normal Linux cross-builder or a native
    # AArch64 builder. On native AArch64, default to the native binutils prefix.
    if [ "$(uname -m)" = "aarch64" ] && [ -z "${CROSS_COMPILE+x}" ]; then
        CROSS_COMPILE=""; export CROSS_COMPILE
    fi
    run "$SCRIPTS/build-kernel.sh"
    run "$SCRIPTS/build-samsung-boot-tools.sh"
    run "$SCRIPTS/make-dt-image.sh"
    run "$SCRIPTS/make-boot-image.sh"
}

case "$PHASE" in
    userspace)
        build_userspace_phase
        ;;
    boot)
        build_boot_phase
        ;;
    package)
        run "$SCRIPTS/package-bundle.sh"
        ;;
    all)
        native_gate
        build_userspace_phase
        build_boot_phase
        run "$SCRIPTS/package-bundle.sh"
        ;;
    *)
        fail "usage: $0 [userspace|boot|package|all]"
        ;;
esac

echo "AuroraOS-S6 phase '$PHASE' complete: $OUT"
