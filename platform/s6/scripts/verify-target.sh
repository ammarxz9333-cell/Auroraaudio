#!/bin/sh
set -eu

fail() {
    echo "AuroraOS-S6 target check: FAIL: $*" >&2
    exit 1
}

[ "$(uname -m)" = "aarch64" ] || fail "AArch64 userspace required; found $(uname -m)"
[ -f /etc/alpine-release ] || fail "AuroraOS-S6 requires Alpine Linux userland"

compatible=""
for f in /proc/device-tree/compatible /sys/firmware/devicetree/base/compatible; do
    if [ -r "$f" ]; then
        compatible="$(tr '\000' ' ' < "$f" 2>/dev/null || true)"
        break
    fi
done

model=""
for f in /proc/device-tree/model /sys/firmware/devicetree/base/model; do
    if [ -r "$f" ]; then
        model="$(tr '\000' ' ' < "$f" 2>/dev/null || true)"
        break
    fi
done

identity="$compatible $model"
echo "$identity" | grep -Eiq 'zeroflte|exynos7420' || \
    fail "unsupported/unverified device tree: ${identity:-unknown}. Initial bring-up target is Galaxy S6 zeroflte/Exynos7420 only"

[ -e /dev/fb0 ] || echo "AuroraOS-S6 target check: WARN: /dev/fb0 not present; LVGL display path needs validation" >&2
ls /dev/input/event* >/dev/null 2>&1 || echo "AuroraOS-S6 target check: WARN: no evdev input nodes found" >&2

if [ -d /sys/class/net ]; then
    wifi="$(find /sys/class/net -maxdepth 1 -type l -printf '%f\n' 2>/dev/null | grep -E '^(wlan|wl)' | head -n1 || true)"
    [ -n "$wifi" ] || echo "AuroraOS-S6 target check: WARN: no Wi-Fi interface detected" >&2
fi

echo "AuroraOS-S6 target check: PASS"
echo "  arch: aarch64"
echo "  alpine: $(cat /etc/alpine-release)"
echo "  device: ${identity:-zeroflte/exynos7420}"
