#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
OUT="${AURORA_OUT:-$ROOT/out/s6-aarch64}"
DIST="$OUT/dist"
STAGE="$DIST/AuroraOS-S6-SM-G920F"
BOOT="$OUT/aurora-s6-boot.img"
SYSTEM="$OUT/AuroraOS-S6-system.ext4"
BUNDLE="$DIST/AuroraOS-S6-SM-G920F-bringup.tar.zst"

fail() { echo "package-bundle: $*" >&2; exit 1; }
for cmd in tar zstd sha256sum stat; do command -v "$cmd" >/dev/null 2>&1 || fail "missing tool: $cmd"; done
for f in "$BOOT" "$SYSTEM" \
         "$OUT/BOOT-IMAGE-MANIFEST.txt" "$OUT/SYSTEM-IMAGE-MANIFEST.txt" \
         "$OUT/BUILD-MANIFEST.txt" \
         "$ROOT/platform/s6/FLASH_GATES.md" \
         "$ROOT/platform/s6/COMPONENT_STATUS.md"; do
    [ -f "$f" ] || fail "missing prerequisite: $f"
done

BOOT_LIMIT=29360128
SYSTEM_LIMIT=3124019200
BOOT_SIZE="$(stat -c '%s' "$BOOT")"
SYSTEM_SIZE="$(stat -c '%s' "$SYSTEM")"
[ "$BOOT_SIZE" -le "$BOOT_LIMIT" ] || fail "boot image exceeds physical BOOT partition"
[ "$SYSTEM_SIZE" -le "$SYSTEM_LIMIT" ] || fail "system image exceeds physical SYSTEM partition"

rm -rf "$STAGE"
mkdir -p "$STAGE/manifests" "$STAGE/docs"
install -m 0644 "$BOOT" "$STAGE/aurora-s6-boot.img"
install -m 0644 "$SYSTEM" "$STAGE/AuroraOS-S6-system.ext4"
install -m 0644 "$OUT/BOOT-IMAGE-MANIFEST.txt" "$STAGE/manifests/"
install -m 0644 "$OUT/SYSTEM-IMAGE-MANIFEST.txt" "$STAGE/manifests/"
install -m 0644 "$OUT/BUILD-MANIFEST.txt" "$STAGE/manifests/"
[ -f "$OUT/kernel/KERNEL-COMMIT.txt" ] && install -m 0644 "$OUT/kernel/KERNEL-COMMIT.txt" "$STAGE/manifests/"
[ -f "$OUT/kernel/DT-IMAGE-MANIFEST.txt" ] && install -m 0644 "$OUT/kernel/DT-IMAGE-MANIFEST.txt" "$STAGE/manifests/"
[ -f "$OUT/tools/SAMSUNG-BOOT-TOOLS-MANIFEST.txt" ] && install -m 0644 "$OUT/tools/SAMSUNG-BOOT-TOOLS-MANIFEST.txt" "$STAGE/manifests/"
install -m 0644 "$ROOT/platform/s6/FLASH_GATES.md" "$STAGE/docs/FLASH_GATES.md"
install -m 0644 "$ROOT/platform/s6/COMPONENT_STATUS.md" "$STAGE/docs/COMPONENT_STATUS.md"
[ -f "$ROOT/docs/AURORA_USB_S6_STM32_PROTOCOL.md" ] && \
    install -m 0644 "$ROOT/docs/AURORA_USB_S6_STM32_PROTOCOL.md" "$STAGE/docs/"

cat > "$STAGE/README-FIRST.txt" <<'EOF'
AuroraOS-S6 bring-up bundle
===========================
Target: Samsung Galaxy S6 SM-G920F / zerofltexx only.
Runtime: Alpine Linux ARM64 + custom Exynos7420 kernel. No Android or postmarketOS runtime.

This archive intentionally contains the BOOT and SYSTEM partition images as
separate files even though delivery is one .tar.zst bundle. The Galaxy S6
bootloader does not consume a single monolithic disk image for both partitions.

READ docs/COMPONENT_STATUS.md before interpreting this archive. Host-tested,
build-script-only, staged third-party, and physical-hardware states are kept
separate on purpose.

DO NOT FLASH this bundle merely because it built successfully.
Physical validation gates in docs/FLASH_GATES.md must be completed first.
In particular, no automatic Odin/Heimdall flashing command is embedded here.

The SYSTEM image may contain firmware copied from the owner's own stock Galaxy
S6 during the build. Those blobs are not stored in the Aurora source repository.
EOF

cat > "$STAGE/NOT_FLASH_READY" <<'EOF'
This marker is deliberate.
Remove it only after the exact physical SM-G920F has passed the documented
boot, display/touch, storage, Wi-Fi/BT, USB FunctionFS, thermal, and recovery
gates. A green host CI build is not a hardware validation.
EOF

(
    cd "$STAGE"
    sha256sum aurora-s6-boot.img AuroraOS-S6-system.ext4 manifests/* docs/* > SHA256SUMS
)

rm -f "$BUNDLE" "$BUNDLE.sha256"
tar -C "$DIST" -cf - "$(basename "$STAGE")" | zstd -T0 -19 -q -o "$BUNDLE"
sha256sum "$BUNDLE" > "$BUNDLE.sha256"

echo "Single delivery bundle built: $BUNDLE"
echo "Status marker retained: NOT_FLASH_READY"
