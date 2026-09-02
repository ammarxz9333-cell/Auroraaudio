#!/bin/sh
set -eu

# Capture only device-owned firmware/configuration needed by AuroraOS-S6.
# Run while the original/compatible Android system partition is still available.
# The captured files are NOT committed to Git and remain local to the owner.

DEST="${1:-./out/s6-stock-firmware}"
mkdir -p "$DEST/wifi" "$DEST/bluetooth"

find_source() {
    for p in "$@"; do
        [ -e "$p" ] && { printf '%s\n' "$p"; return 0; }
    done
    return 1
}

copy_if_present() {
    src="$1"
    dst="$2"
    if [ -f "$src" ]; then
        install -m 0644 "$src" "$dst/$(basename "$src")"
        return 0
    fi
    return 1
}

# Samsung universal7420 device configuration switches bcmdhd firmware at runtime
# between the station and AP/STA images. Preserve both when present.
WIFI_DIR="$(find_source /system/vendor/etc/wifi /vendor/etc/wifi /system/etc/wifi 2>/dev/null || true)"
if [ -n "$WIFI_DIR" ]; then
    copy_if_present "$WIFI_DIR/bcmdhd_sta.bin" "$DEST/wifi" || true
    copy_if_present "$WIFI_DIR/bcmdhd_apsta.bin" "$DEST/wifi" || true
    # Preserve board-specific nvram/config files without assuming their exact stock names.
    find "$WIFI_DIR" -maxdepth 1 -type f \( -iname '*nvram*' -o -iname '*bcmdhd*.conf' -o -iname '*wifi*.conf' \) -exec cp -p '{}' "$DEST/wifi/" ';' 2>/dev/null || true
fi

for d in /system/vendor/firmware /vendor/firmware /system/etc/firmware; do
    [ -d "$d" ] || continue
    for f in "$d"/bcm4358*.hcd "$d"/bcm4359*.hcd; do
        [ -f "$f" ] || continue
        copy_if_present "$f" "$DEST/bluetooth" || true
    done
done

WIFI_COUNT="$(find "$DEST/wifi" -type f 2>/dev/null | wc -l | tr -d ' ')"
BT_COUNT="$(find "$DEST/bluetooth" -type f 2>/dev/null | wc -l | tr -d ' ')"

[ "$WIFI_COUNT" -gt 0 ] || {
    echo "capture-stock-firmware: no Wi-Fi firmware found; refusing to produce a Wi-Fi-capable Aurora image" >&2
    exit 1
}

{
    echo "captured_from=$(date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || echo unknown)"
    echo "wifi_files=$WIFI_COUNT"
    echo "bluetooth_files=$BT_COUNT"
} > "$DEST/CAPTURE-MANIFEST.txt"

if command -v sha256sum >/dev/null 2>&1; then
    (cd "$DEST" && find wifi bluetooth -type f -print0 | sort -z | xargs -0 sha256sum) > "$DEST/SHA256SUMS"
fi

echo "Captured private S6 firmware into: $DEST"
echo "Do not commit this directory; it is owner-supplied device firmware."
