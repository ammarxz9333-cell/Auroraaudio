#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
INIT_DIR="$ROOT/platform/s6/rootfs/etc/init.d"
LIVE="$ROOT/platform/s6/live-ingest/aurora-live-ingest.c"
POST="$ROOT/crates/aurora-cli/src/bin/aurora-s6-postprocess.rs"
BUILD="$ROOT/platform/s6/scripts/build-userspace-native-aarch64.sh"
ASSEMBLE="$ROOT/platform/s6/scripts/assemble-rootfs-native-aarch64.sh"
ENVFILE="$ROOT/platform/s6/rootfs/etc/aurora/aurora.env"
HW_TARGET="$ROOT/config/aurora-hardware-target.env"
HW_HEADER_GENERATOR="$ROOT/firmware/generate-hardware-target-header.sh"
PINMUX_PROOF="$ROOT/docs/AURORA_REALTIME_MCU_PINMUX_PROOF.md"
USB_PROTOCOL_DOC="$ROOT/docs/AURORA_USB_S6_REALTIME_MCU_PROTOCOL.md"
EARC_BRINGUP_DOC="$ROOT/docs/AURORA_EARC_REALTIME_MCU_PHYSICAL_BRINGUP.md"
SERVICE="$INIT_DIR/aurora-live-ingest"
FFS_SERVICE="$INIT_DIR/aurora-ffs"

fail() {
    echo "audit-runtime-wiring: FAIL: $*" >&2
    exit 1
}

count_fixed() {
    pattern="$1"
    file="$2"
    grep -F -c -- "$pattern" "$file" || true
}

env_value() {
    key="$1"
    sed -n "s/^${key}=//p" "$ENVFILE"
}

# ---------------------------------------------------------------------------
# Single hardware source of truth
# ---------------------------------------------------------------------------
[ -f "$HW_TARGET" ] || fail "hardware target manifest missing"
# shellcheck disable=SC1090
. "$HW_TARGET"

for var in \
    AURORA_REALTIME_MCU_ROLE AURORA_REALTIME_MCU_VENDOR \
    AURORA_REALTIME_MCU_FAMILY AURORA_REALTIME_MCU_PART \
    AURORA_REALTIME_MCU_PACKAGE AURORA_REALTIME_MCU_SOURCE_DIR \
    AURORA_REALTIME_MCU_VENDOR_STACK AURORA_REALTIME_MCU_VENDOR_STACK_VERSION \
    AURORA_REALTIME_MCU_VENDOR_HAL_COMMIT AURORA_REALTIME_MCU_VENDOR_USB_HOST_COMMIT \
    AURORA_REALTIME_MCU_VENDOR_DEVICE_COMMIT \
    AURORA_USB_PHY_PART AURORA_USB_VBUS_SWITCH_PART
 do
    eval "value=\${$var-}"
    [ -n "$value" ] || fail "required hardware field missing: $var"
 done

[ "$AURORA_REALTIME_MCU_ROLE" = aurora-realtime-mcu ] || fail "unexpected realtime MCU role"
[ "$AURORA_REALTIME_MCU_SOURCE_DIR" = firmware/realtime-mcu ] || fail "portable MCU source path must remain target-neutral"
[ -d "$ROOT/$AURORA_REALTIME_MCU_SOURCE_DIR/include" ] || fail "MCU include directory missing"
[ -d "$ROOT/$AURORA_REALTIME_MCU_SOURCE_DIR/src" ] || fail "MCU source directory missing"
[ -d "$ROOT/$AURORA_REALTIME_MCU_SOURCE_DIR/test" ] || fail "MCU test directory missing"

[ "$AURORA_REALTIME_MCU_USB_HS_HOST" = 1 ] || fail "USB HS host is mandatory"
[ "$AURORA_REALTIME_MCU_USB_HS_PHY" = ULPI ] || fail "ULPI is mandatory"
[ "$AURORA_REALTIME_MCU_ULPI_DIRECT_PINS" = 1 ] || fail "direct ULPI pins are mandatory"
[ "$AURORA_REALTIME_MCU_SAI_RX" = 1 ] || fail "serial-audio RX is mandatory"
[ "$AURORA_REALTIME_MCU_SAI_TDM_TX" = 1 ] || fail "TDM TX is mandatory"
[ "$AURORA_REALTIME_MCU_DMA" = 1 ] || fail "DMA is mandatory"
[ "$AURORA_REALTIME_MCU_SAMPLE_RATE" = 48000 ] || fail "hardware sample-rate drift"
[ "$AURORA_REALTIME_MCU_CHANNELS" = 12 ] || fail "hardware channel-count drift"
[ "$AURORA_REALTIME_MCU_PERIOD_FRAMES" = 40 ] || fail "hardware period drift"
[ "$AURORA_REALTIME_MCU_PINMUX_STATUS" = datasheet_verified ] || fail "pinmux not datasheet-verified"
[ "$AURORA_REALTIME_MCU_HAL_STATUS" = not_implemented ] || fail "HAL status changed without acceptance update"
[ "$AURORA_REALTIME_MCU_PHYSICAL_STATUS" = not_measured ] || fail "physical status changed without measured evidence"
[ -n "$AURORA_REALTIME_MCU_PINMUX_PROOF" ] || fail "pinmux proof identifier missing"
[ -f "$PINMUX_PROOF" ] || fail "pinmux proof document missing"
[ -f "$USB_PROTOCOL_DOC" ] || fail "target-neutral USB protocol document missing"
[ -f "$EARC_BRINGUP_DOC" ] || fail "target-neutral eARC bring-up document missing"

# Old active MCU identifiers are forbidden. Historical/research material is not
# rewritten, but current source/build/runtime/contracts may never depend on them.
[ ! -e "$ROOT/firmware/stm32h753" ] || fail "legacy firmware/stm32h753 tree still exists"
[ ! -e "$ROOT/docs/AURORA_USB_S6_STM32_PROTOCOL.md" ] || fail "legacy USB protocol filename still exists"
[ ! -e "$ROOT/docs/AURORA_EARC_STM32_PHYSICAL_BRINGUP.md" ] || fail "legacy eARC bring-up filename still exists"
for legacy in \
    stm32h753 STM32H753 aurora_stm32_audio_app AURORA_STM32_ \
    AURORA_STM32CUBE_ AURORA_USB_S6_STM32_PROTOCOL AURORA_EARC_STM32_PHYSICAL_BRINGUP
 do
    if grep -R -n -F --exclude-dir=.git -- "$legacy" \
        "$ROOT/.github" "$ROOT/config" "$ROOT/firmware" \
        "$ROOT/platform" "$ROOT/protocol" >/tmp/aurora-legacy-name-hit 2>/dev/null; then
        cat /tmp/aurora-legacy-name-hit >&2
        rm -f /tmp/aurora-legacy-name-hit
        fail "legacy active MCU identifier remains: $legacy"
    fi
 done
rm -f /tmp/aurora-legacy-name-hit

# USB data + power contract for the S6 gadget.
[ "$AURORA_USB_PHY_INTERFACE" = ULPI ] || fail "USB PHY interface drift"
[ "$AURORA_USB_PHY_SPEED_MBIT" -ge 480 ] || fail "USB PHY is not high-speed"
[ "$AURORA_USB_PHY_BC_SPEC" = BC1.2 ] || fail "BC1.2 support required"
[ "$AURORA_USB_PHY_PORT_MODE" = CDP ] || fail "charging downstream port required"
[ "$AURORA_USB_PHY_CDP_CURRENT_MA" -ge 1500 ] || fail "CDP current below 1.5 A"
[ "$AURORA_USB_PHY_ULPI_CLOCK_HZ" = 60000000 ] || fail "ULPI clock must be 60 MHz"
[ "$AURORA_USB_PHY_REFERENCE_HZ" = 24000000 ] || fail "PHY reference must be 24 MHz"
[ "$AURORA_USB_PHY_REFSEL_VALUE" = 7 ] || fail "PHY REFSEL must be numeric 7 (binary 111)"
[ "$AURORA_USB_PHY_RESET_ACTIVE_LOW" = 1 ] || fail "PHY reset polarity drift"
[ "$AURORA_USB_PHY_VBUS_ENABLE_SIGNAL" = CPEN ] || fail "PHY VBUS enable signal drift"
[ "$AURORA_USB_PHY_STATUS" = datasheet_verified ] || fail "USB PHY is not datasheet-verified"

[ "$AURORA_USB_VBUS_TARGET_MA" -ge 1500 ] || fail "VBUS target below 1.5 A"
[ "$AURORA_USB_VBUS_SWITCH_MAX_CONTINUOUS_MA" -ge "$AURORA_USB_VBUS_TARGET_MA" ] || fail "VBUS switch undersized"
[ "$AURORA_USB_VBUS_SWITCH_ADJUSTABLE_ILIM" = 1 ] || fail "VBUS current limiting required"
[ "$AURORA_USB_VBUS_SWITCH_REVERSE_BLOCKING" = 1 ] || fail "VBUS reverse blocking required"
[ "$AURORA_USB_VBUS_SWITCH_FAULT_OUTPUT" = 1 ] || fail "VBUS fault output required"
[ "$AURORA_USB_VBUS_STATUS" = datasheet_verified ] || fail "VBUS switch is not datasheet-verified"

# No GPIO may own two live roles.
duplicate_pins="$(sed -n 's/^AURORA_PIN_[^=]*=//p' "$HW_TARGET" | sort | uniq -d)"
[ -z "$duplicate_pins" ] || fail "duplicate realtime-MCU pin assignments: $duplicate_pins"

# Pin AF groups are target contract, not copied into HAL source.
for v in \
    AURORA_AF_ULPI_STP AURORA_AF_ULPI_DIR AURORA_AF_ULPI_NXT \
    AURORA_AF_ULPI_CLK AURORA_AF_ULPI_D0 AURORA_AF_ULPI_D1 \
    AURORA_AF_ULPI_D2 AURORA_AF_ULPI_D3 AURORA_AF_ULPI_D4 \
    AURORA_AF_ULPI_D5 AURORA_AF_ULPI_D6 AURORA_AF_ULPI_D7
 do
    eval "value=\${$v-}"
    [ "$value" = 10 ] || fail "$v must be AF10"
 done
for v in AURORA_AF_EARC_FS AURORA_AF_EARC_SCK AURORA_AF_EARC_SD; do
    eval "value=\${$v-}"
    [ "$value" = 6 ] || fail "$v must be AF6"
done
for v in AURORA_AF_TDM_FS AURORA_AF_TDM_SCK AURORA_AF_TDM_SD AURORA_AF_TDM_MCLK; do
    eval "value=\${$v-}"
    [ "$value" = 10 ] || fail "$v must be AF10"
done

# HAL-facing constants must be generated deterministically from the manifest.
[ -f "$HW_HEADER_GENERATOR" ] || fail "hardware target header generator missing"
sh -n "$HW_HEADER_GENERATOR" || fail "hardware target header generator syntax error"
generated_header="$(mktemp)"
trap 'rm -f "$generated_header"' EXIT HUP INT TERM
sh "$HW_HEADER_GENERATOR" "$generated_header" || fail "hardware target header generation failed"
for expected in \
    "#define AURORA_HW_MCU_PART \"$AURORA_REALTIME_MCU_PART\"" \
    "#define AURORA_HW_MCU_PACKAGE \"$AURORA_REALTIME_MCU_PACKAGE\"" \
    '#define AURORA_HW_SAMPLE_RATE_HZ 48000u' \
    '#define AURORA_HW_CHANNELS 12u' \
    '#define AURORA_HW_PERIOD_FRAMES 40u' \
    '#define AURORA_HW_USB_PHY_CDP_CURRENT_MA 1500u' \
    '#define AURORA_HW_USB_PHY_ULPI_CLOCK_HZ 60000000u' \
    '#define AURORA_HW_USB_PHY_REFERENCE_HZ 24000000u' \
    '#define AURORA_HW_USB_PHY_REFSEL_VALUE 7u' \
    '#define AURORA_HW_USB_VBUS_TARGET_MA 1500u'
 do
    grep -Fq "$expected" "$generated_header" || fail "generated header missing: $expected"
 done
for macro in \
    AURORA_HW_ULPI_DIR_AF AURORA_HW_ULPI_NXT_AF \
    AURORA_HW_EARC_FS_AF AURORA_HW_EARC_SCK_AF AURORA_HW_EARC_SD_AF \
    AURORA_HW_TDM_FS_AF AURORA_HW_TDM_SCK_AF AURORA_HW_TDM_SD_AF \
    AURORA_HW_USB_PHY_RESET_PIN_NUMBER AURORA_HW_USB_VBUS_FAULT_PIN_NUMBER \
    AURORA_HW_AMP_MUTE_PIN_NUMBER
 do
    grep -Fq "#define $macro " "$generated_header" || fail "generated header missing $macro"
 done

# Concrete selected part must not leak into runtime or architecture consumers.
for file in \
    "$LIVE" "$POST" "$BUILD" "$ASSEMBLE" "$SERVICE" "$FFS_SERVICE" \
    "$ROOT/.github/workflows/s6-appliance-ci.yml" \
    "$USB_PROTOCOL_DOC" "$EARC_BRINGUP_DOC" \
    "$ROOT/docs/AURORA_SYSTEM_ARCHITECTURE_HARDENING_CONTRACT.md" \
    "$PINMUX_PROOF"
 do
    grep -Fq -- "$AURORA_REALTIME_MCU_PART" "$file" && fail "concrete MCU part leaked into $file"
 done

# ---------------------------------------------------------------------------
# One appliance runtime chain
# ---------------------------------------------------------------------------
set -- "$INIT_DIR"/aurora-*
[ "$#" -eq 2 ] || fail "expected exactly two Aurora init services, found $#"
[ -f "$FFS_SERVICE" ] || fail "aurora-ffs service missing"
[ -f "$SERVICE" ] || fail "aurora-live-ingest service missing"
sh -n "$FFS_SERVICE" || fail "aurora-ffs syntax error"
sh -n "$SERVICE" || fail "aurora-live-ingest syntax error"
[ "$(count_fixed 'need aurora-ffs' "$SERVICE")" -eq 1 ] || fail "live ingest dependency duplicated/missing"

[ "$(count_fixed '. /etc/aurora/aurora.env' "$SERVICE")" -eq 1 ] || fail "runtime manifest must be sourced exactly once"
[ "$(count_fixed 'command="$AURORA_LIVE_INGEST_BIN"' "$SERVICE")" -eq 1 ] || fail "live command must come from runtime manifest"
[ "$(count_fixed 'AURORA_CONFIG_MISSING=1' "$SERVICE")" -eq 1 ] || fail "missing-config guard absent"
[ "$(count_fixed '[ "$AURORA_CONFIG_MISSING" -eq 0 ] || return 1' "$SERVICE")" -eq 1 ] || fail "service is not fail-closed on missing config"

for assignment in \
    'AURORA_LIVE_INGEST_BIN=/usr/local/sbin/aurora-live-ingest' \
    'AURORA_POSTPROCESS_BIN=/usr/local/bin/aurora-s6-postprocess' \
    'AURORA_ORENDER_BIN=/opt/aurora/external/orender' \
    'AURORA_HARLETTY_BRIDGE=/opt/aurora/external/libharletty_bridge.so' \
    'AURORA_7_1_4_LAYOUT=/etc/aurora/layouts/7.1.4.yaml'
 do
    [ "$(count_fixed "$assignment" "$ENVFILE")" -eq 1 ] || fail "runtime assignment missing/duplicated: $assignment"
 done
if grep -Eq '^(HARLETTY_BIN|OMNIPHONY_BIN)=' "$ENVFILE"; then
    fail "obsolete runtime path aliases remain"
fi

broker_orender="$(sed -n 's/^#define DEFAULT_ORENDER "\(.*\)"$/\1/p' "$LIVE")"
broker_harletty="$(sed -n 's/^#define DEFAULT_HARLETTY "\(.*\)"$/\1/p' "$LIVE")"
broker_layout="$(sed -n 's/^#define DEFAULT_LAYOUT "\(.*\)"$/\1/p' "$LIVE")"
broker_post="$(sed -n 's/^#define DEFAULT_POSTPROCESS "\(.*\)"$/\1/p' "$LIVE")"
[ "$broker_orender" = "$(env_value AURORA_ORENDER_BIN)" ] || fail "Omniphony fallback drift"
[ "$broker_harletty" = "$(env_value AURORA_HARLETTY_BRIDGE)" ] || fail "Harletty fallback drift"
[ "$broker_layout" = "$(env_value AURORA_7_1_4_LAYOUT)" ] || fail "layout fallback drift"
[ "$broker_post" = "$(env_value AURORA_POSTPROCESS_BIN)" ] || fail "postprocessor fallback drift"

for install_line in \
    'install -m 0755 "$STAGE/bin/aurora-live-ingest" "$ROOTFS/usr/local/sbin/aurora-live-ingest"' \
    'install -m 0755 "$STAGE/bin/aurora-s6-postprocess" "$ROOTFS/usr/local/bin/aurora-s6-postprocess"' \
    'install -m 0755 "$STAGE/bin/aurora-ffs-daemon" "$ROOTFS/usr/local/sbin/aurora-ffs-daemon"' \
    'install -m 0755 "$STAGE/bin/orender" "$ROOTFS/opt/aurora/external/orender"' \
    'install -m 0755 "$STAGE/lib/libharletty_bridge.so" "$ROOTFS/opt/aurora/external/libharletty_bridge.so"' \
    'install -m 0644 "$STAGE/share/omniphony/layouts/7.1.4.yaml" "$ROOTFS/etc/aurora/layouts/7.1.4.yaml'
 do
    [ "$(count_fixed "$install_line" "$ASSEMBLE")" -eq 1 ] || fail "rootfs install missing/duplicated: $install_line"
 done
[ "$(count_fixed 'ln -sf /etc/init.d/aurora-ffs "$ROOTFS/etc/runlevels/default/aurora-ffs"' "$ASSEMBLE")" -eq 1 ] || fail "aurora-ffs not enabled exactly once"
[ "$(count_fixed 'ln -sf /etc/init.d/aurora-live-ingest "$ROOTFS/etc/runlevels/default/aurora-live-ingest"' "$ASSEMBLE")" -eq 1 ] || fail "live-ingest not enabled exactly once"

# One renderer, one DSP/postprocessor, one shared ASRC/drift implementation.
[ "$(count_fixed 'execl(orender, orender,' "$LIVE")" -eq 1 ] || fail "Omniphony exec path duplicated/missing"
[ "$(count_fixed 'execl(postprocess, postprocess,' "$LIVE")" -eq 1 ] || fail "postprocessor exec path duplicated/missing"
if grep -R -i -q -- 'camilladsp' "$INIT_DIR"; then
    fail "CamillaDSP must not run in parallel on S6"
fi
grep -Fq -- 'AsynchronousResampler, DriftController, DriftControllerConfig, RubatoAsrc' "$POST" || fail "postprocessor is not using shared realtime clock primitives"
if grep -Eq -- '^[[:space:]]*(pub[[:space:]]+)?struct[[:space:]]+(RubatoAsrc|DriftController)[[:space:]]*\{' "$POST"; then
    fail "postprocessor redefines ASRC/drift primitive"
fi

# Version and stream-shape truth must stay consistent.
env_harletty="$(sed -n 's/^HARLETTY_VERSION=//p' "$ENVFILE")"
build_harletty="$(sed -n 's/^HARLETTY_VERSION="${HARLETTY_VERSION:-\(.*\)}"$/\1/p' "$BUILD")"
[ -n "$env_harletty" ] && [ "$env_harletty" = "$build_harletty" ] || fail "Harletty version drift"
env_omniphony="$(sed -n 's/^OMNIPHONY_VERSION=//p' "$ENVFILE")"
build_omniphony="$(sed -n 's/^OMNIPHONY_VERSION="${OMNIPHONY_VERSION:-\(.*\)}"$/\1/p' "$BUILD")"
[ -n "$env_omniphony" ] && [ "$env_omniphony" = "$build_omniphony" ] || fail "Omniphony version drift"
env_navidrome="$(sed -n 's/^NAVIDROME_VERSION=//p' "$ENVFILE")"
build_navidrome="$(sed -n 's/^NAVIDROME_VERSION="${NAVIDROME_VERSION:-\(.*\)}"$/\1/p' "$BUILD")"
[ -n "$env_navidrome" ] && [ "$env_navidrome" = "$build_navidrome" ] || fail "Navidrome version drift"

grep -Fq 'AURORA_SAMPLE_RATE=48000' "$ENVFILE" || fail "runtime sample-rate drift"
grep -Fq 'AURORA_BLOCK_FRAMES=40' "$ENVFILE" || fail "runtime block-size drift"
grep -Fq 'AURORA_LAYOUT=7.1.4' "$ENVFILE" || fail "runtime layout drift"

echo "audit-runtime-wiring: PASS single hardware manifest, no legacy active MCU names, generated HAL constants, unique pin map, protected USB power/data path, single renderer/DSP/ASRC chain, canonical runtime config"
