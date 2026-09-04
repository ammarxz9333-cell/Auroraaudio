#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
INIT_DIR="$ROOT/platform/s6/rootfs/etc/init.d"
LIVE="$ROOT/platform/s6/live-ingest/aurora-live-ingest.c"
SOURCE_MANAGER="$ROOT/platform/s6/source-manager/aurora-source-manager.c"
SOURCE_GATE="$ROOT/platform/s6/source-manager/aurora-source-gate.c"
POST="$ROOT/crates/aurora-cli/src/bin/aurora-s6-postprocess.rs"
BUILD="$ROOT/platform/s6/scripts/build-userspace-native-aarch64.sh"
ASSEMBLE="$ROOT/platform/s6/scripts/assemble-rootfs-native-aarch64.sh"
ENVFILE="$ROOT/platform/s6/rootfs/etc/aurora/aurora.env"
HW_TARGET="$ROOT/config/aurora-hardware-target.env"
HW_HEADER_GENERATOR="$ROOT/firmware/generate-hardware-target-header.sh"
PINMUX_PROOF="$ROOT/docs/AURORA_REALTIME_MCU_PINMUX_PROOF.md"
USB_PROTOCOL_DOC="$ROOT/docs/AURORA_USB_S6_REALTIME_MCU_PROTOCOL.md"
EARC_BRINGUP_DOC="$ROOT/docs/AURORA_EARC_REALTIME_MCU_PHYSICAL_BRINGUP.md"
LIVE_SERVICE="$INIT_DIR/aurora-live-ingest"
SOURCE_MANAGER_SERVICE="$INIT_DIR/aurora-source-manager"
SOURCE_GATE_SERVICE="$INIT_DIR/aurora-source-gate"
FFS_SERVICE="$INIT_DIR/aurora-ffs"

fail() {
    echo "audit-runtime-wiring: FAIL: $*" >&2
    exit 1
}

count_fixed() {
    grep -F -c -- "$1" "$2" 2>/dev/null || true
}

env_value() {
    sed -n "s/^$1=//p" "$ENVFILE"
}

# ---------------------------------------------------------------------------
# Hardware target: one manifest, target-neutral portable code.
# ---------------------------------------------------------------------------
[ -f "$HW_TARGET" ] || fail "hardware target manifest missing"
# shellcheck disable=SC1090
. "$HW_TARGET"

for var in \
    AURORA_REALTIME_MCU_ROLE AURORA_REALTIME_MCU_VENDOR \
    AURORA_REALTIME_MCU_FAMILY AURORA_REALTIME_MCU_PART \
    AURORA_REALTIME_MCU_PACKAGE AURORA_REALTIME_MCU_SOURCE_DIR \
    AURORA_REALTIME_MCU_USB_HS_HOST AURORA_REALTIME_MCU_USB_HS_PHY \
    AURORA_REALTIME_MCU_ULPI_DIRECT_PINS AURORA_REALTIME_MCU_SAI_RX \
    AURORA_REALTIME_MCU_SAI_TDM_TX AURORA_REALTIME_MCU_DMA \
    AURORA_USB_PHY_PART AURORA_USB_VBUS_SWITCH_PART
 do
    eval "value=\${$var-}"
    [ -n "$value" ] || fail "required hardware field missing: $var"
 done

[ "$AURORA_REALTIME_MCU_ROLE" = aurora-realtime-mcu ] || fail "unexpected realtime MCU role"
[ "$AURORA_REALTIME_MCU_SOURCE_DIR" = firmware/realtime-mcu ] || fail "portable MCU source path drift"
[ -d "$ROOT/$AURORA_REALTIME_MCU_SOURCE_DIR/include" ] || fail "MCU include directory missing"
[ -d "$ROOT/$AURORA_REALTIME_MCU_SOURCE_DIR/src" ] || fail "MCU source directory missing"
[ -d "$ROOT/$AURORA_REALTIME_MCU_SOURCE_DIR/test" ] || fail "MCU test directory missing"
[ "$AURORA_REALTIME_MCU_USB_HS_HOST" = 1 ] || fail "USB HS host required"
[ "$AURORA_REALTIME_MCU_USB_HS_PHY" = ULPI ] || fail "ULPI required"
[ "$AURORA_REALTIME_MCU_ULPI_DIRECT_PINS" = 1 ] || fail "direct ULPI pins required"
[ "$AURORA_REALTIME_MCU_SAI_RX" = 1 ] || fail "serial-audio RX required"
[ "$AURORA_REALTIME_MCU_SAI_TDM_TX" = 1 ] || fail "TDM TX required"
[ "$AURORA_REALTIME_MCU_DMA" = 1 ] || fail "DMA required"
[ "$AURORA_REALTIME_MCU_SAMPLE_RATE" = 48000 ] || fail "hardware sample-rate drift"
[ "$AURORA_REALTIME_MCU_CHANNELS" = 12 ] || fail "hardware channel-count drift"
[ "$AURORA_REALTIME_MCU_PERIOD_FRAMES" = 40 ] || fail "hardware period drift"
[ "$AURORA_REALTIME_MCU_PINMUX_STATUS" = datasheet_verified ] || fail "pinmux not datasheet-verified"
[ "$AURORA_REALTIME_MCU_HAL_STATUS" = not_implemented ] || fail "HAL status changed without acceptance update"
[ "$AURORA_REALTIME_MCU_PHYSICAL_STATUS" = not_measured ] || fail "physical status changed without measured evidence"
[ -f "$PINMUX_PROOF" ] || fail "pinmux proof document missing"
[ -f "$USB_PROTOCOL_DOC" ] || fail "USB protocol document missing"
[ -f "$EARC_BRINGUP_DOC" ] || fail "eARC bring-up document missing"

# Historical concrete MCU names may exist in research/history, but never in the
# active build/runtime surfaces. This audit file is excluded because it carries
# the deny-list literals itself.
for legacy in \
    stm32h753 STM32H753 aurora_stm32_audio_app AURORA_STM32_ \
    AURORA_STM32CUBE_ AURORA_USB_S6_STM32_PROTOCOL AURORA_EARC_STM32_PHYSICAL_BRINGUP
 do
    if grep -R -n -F --exclude-dir=.git --exclude=audit-runtime-wiring.sh -- "$legacy" \
        "$ROOT/.github" "$ROOT/config" "$ROOT/firmware" \
        "$ROOT/platform" "$ROOT/protocol" "$ROOT/docs" \
        >/tmp/aurora-legacy-name-hit 2>/dev/null; then
        cat /tmp/aurora-legacy-name-hit >&2
        rm -f /tmp/aurora-legacy-name-hit
        fail "legacy active MCU identifier remains: $legacy"
    fi
 done
rm -f /tmp/aurora-legacy-name-hit

[ "$AURORA_USB_PHY_INTERFACE" = ULPI ] || fail "USB PHY interface drift"
[ "$AURORA_USB_PHY_SPEED_MBIT" -ge 480 ] || fail "USB PHY is not high-speed"
[ "$AURORA_USB_PHY_PORT_MODE" = CDP ] || fail "USB charging/data port must remain CDP"
[ "$AURORA_USB_PHY_CDP_CURRENT_MA" -ge 1500 ] || fail "CDP current below 1.5 A"
[ "$AURORA_USB_VBUS_TARGET_MA" -ge 1500 ] || fail "VBUS target below 1.5 A"
[ "$AURORA_USB_VBUS_SWITCH_ADJUSTABLE_ILIM" = 1 ] || fail "VBUS current limiting required"
[ "$AURORA_USB_VBUS_SWITCH_REVERSE_BLOCKING" = 1 ] || fail "VBUS reverse blocking required"
[ "$AURORA_USB_VBUS_SWITCH_FAULT_OUTPUT" = 1 ] || fail "VBUS fault output required"

duplicate_pins="$(sed -n 's/^AURORA_PIN_[^=]*=//p' "$HW_TARGET" | sort | uniq -d)"
[ -z "$duplicate_pins" ] || fail "duplicate realtime-MCU pin assignments: $duplicate_pins"

[ -f "$HW_HEADER_GENERATOR" ] || fail "hardware target header generator missing"
sh -n "$HW_HEADER_GENERATOR" || fail "hardware header generator syntax error"
generated_header="$(mktemp)"
trap 'rm -f "$generated_header"' EXIT HUP INT TERM
sh "$HW_HEADER_GENERATOR" "$generated_header" || fail "hardware header generation failed"
for expected in \
    "#define AURORA_HW_MCU_PART \"$AURORA_REALTIME_MCU_PART\"" \
    '#define AURORA_HW_SAMPLE_RATE_HZ 48000u' \
    '#define AURORA_HW_CHANNELS 12u' \
    '#define AURORA_HW_PERIOD_FRAMES 40u'
 do
    grep -Fq "$expected" "$generated_header" || fail "generated hardware header missing: $expected"
 done

for file in \
    "$LIVE" "$SOURCE_MANAGER" "$SOURCE_GATE" "$POST" "$BUILD" "$ASSEMBLE" \
    "$LIVE_SERVICE" "$SOURCE_MANAGER_SERVICE" "$SOURCE_GATE_SERVICE" "$FFS_SERVICE" \
    "$ROOT/.github/workflows/s6-appliance-ci.yml" "$USB_PROTOCOL_DOC" \
    "$EARC_BRINGUP_DOC" "$ROOT/docs/AURORA_SYSTEM_ARCHITECTURE_HARDENING_CONTRACT.md" \
    "$PINMUX_PROOF"
 do
    grep -Fq -- "$AURORA_REALTIME_MCU_PART" "$file" && fail "concrete MCU part leaked into $file"
 done

# ---------------------------------------------------------------------------
# Runtime topology: one FunctionFS backend, one final source mux.
# ---------------------------------------------------------------------------
set -- "$INIT_DIR"/aurora-*
[ "$#" -eq 4 ] || fail "expected exactly four Aurora init services, found $#"
for service in "$FFS_SERVICE" "$SOURCE_MANAGER_SERVICE" "$SOURCE_GATE_SERVICE" "$LIVE_SERVICE"; do
    [ -f "$service" ] || fail "required service missing: $service"
    sh -n "$service" || fail "service syntax error: $service"
done

[ "$(count_fixed 'need aurora-ffs aurora-source-manager' "$SOURCE_GATE_SERVICE")" -eq 1 ] || \
    fail "source gate must depend on FunctionFS and source manager exactly once"
[ "$(count_fixed 'need aurora-source-gate' "$LIVE_SERVICE")" -eq 1 ] || \
    fail "live ingest must depend on source gate exactly once"
if grep -Fq 'need aurora-ffs' "$LIVE_SERVICE"; then
    fail "live ingest must not depend directly on FunctionFS"
fi

for service in "$SOURCE_MANAGER_SERVICE" "$SOURCE_GATE_SERVICE" "$LIVE_SERVICE"; do
    [ "$(count_fixed '. /etc/aurora/aurora.env' "$service")" -eq 1 ] || \
        fail "service must source runtime manifest exactly once: $service"
    [ "$(count_fixed 'AURORA_CONFIG_MISSING=1' "$service")" -eq 1 ] || \
        fail "missing-config guard absent: $service"
done

for assignment in \
    'AURORA_SOURCE_MANAGER_BIN=/usr/local/sbin/aurora-source-manager' \
    'AURORA_SOURCE_GATE_BIN=/usr/local/sbin/aurora-source-gate' \
    'AURORA_SOURCE_MANAGER_SOCKET=/run/aurora/source-manager.sock' \
    'AURORA_HDMI_SOURCE_SOCKET=/run/aurora/hdmi-source.sock' \
    'AURORA_LOCAL_SOURCE_SOCKET=/run/aurora/local-source.sock' \
    'AURORA_USB_BRIDGE_SOCKET=/run/aurora/hdmi-source.sock' \
    'AURORA_USB_BRIDGE_SOCKET_REAL=/run/aurora/usb-bridge.sock' \
    'AURORA_LIVE_INGEST_BIN=/usr/local/sbin/aurora-live-ingest' \
    'AURORA_POSTPROCESS_BIN=/usr/local/bin/aurora-s6-postprocess' \
    'AURORA_ORENDER_BIN=/opt/aurora/external/orender' \
    'AURORA_HARLETTY_BRIDGE=/opt/aurora/external/libharletty_bridge.so' \
    'AURORA_7_1_4_LAYOUT=/etc/aurora/layouts/7.1.4.yaml'
 do
    [ "$(count_fixed "$assignment" "$ENVFILE")" -eq 1 ] || \
        fail "runtime assignment missing/duplicated: $assignment"
 done

[ "$(env_value AURORA_USB_BRIDGE_SOCKET)" = "$(env_value AURORA_HDMI_SOURCE_SOCKET)" ] || \
    fail "live ingest does not terminate at source gate"
[ "$(env_value AURORA_USB_BRIDGE_SOCKET_REAL)" = /run/aurora/usb-bridge.sock ] || \
    fail "source gate real bridge path drift"

# Semantic mux checks: do not depend on C line wrapping. The gate owns the only
# real FunctionFS path and independently registers both HDMI and Local with the
# source manager. CONFIG is cached per source and PCM requires grant + CONFIG ACK.
[ "$(count_fixed 'AURORA_USB_BRIDGE_SOCKET_REAL' "$SOURCE_GATE")" -eq 1 ] || \
    fail "gate must own exactly one real FunctionFS bridge environment path"
[ "$(count_fixed 'AURORA_HDMI_SOURCE_SOCKET' "$SOURCE_GATE")" -eq 1 ] || \
    fail "HDMI source socket ownership drift"
[ "$(count_fixed 'AURORA_LOCAL_SOURCE_SOCKET' "$SOURCE_GATE")" -eq 1 ] || \
    fail "Local source socket ownership drift"
[ "$(count_fixed 'AURORA_SOURCE_HDMI_EARC' "$SOURCE_GATE")" -ge 1 ] || \
    fail "gate does not model HDMI source ownership"
[ "$(count_fixed 'AURORA_SOURCE_LOCAL_MUSIC' "$SOURCE_GATE")" -ge 1 ] || \
    fail "gate does not model Local source ownership"
[ "$(count_fixed 'struct source_slot' "$SOURCE_GATE")" -ge 2 ] || \
    fail "gate does not maintain per-source ownership state"
[ "$(count_fixed 'config_awaiting_ack' "$SOURCE_GATE")" -ge 4 ] || \
    fail "gate does not enforce CONFIG ACK state"
[ "$(count_fixed 'config_cached' "$SOURCE_GATE")" -ge 3 ] || \
    fail "gate does not cache source CONFIG"
[ "$(count_fixed '!slot->granted' "$SOURCE_GATE")" -ge 2 ] || \
    fail "final mux has no explicit inactive-source drop gate"
[ "$(count_fixed 'AURORA_SOURCE_QUIESCED' "$SOURCE_GATE")" -ge 1 ] || \
    fail "source gate has no quiesce acknowledgement"
[ "$(count_fixed 'AURORA_SOURCE_REVOKE' "$SOURCE_MANAGER")" -ge 1 ] || \
    fail "source manager has no revoke transition"

if grep -Eq '^(HARLETTY_BIN|OMNIPHONY_BIN)=' "$ENVFILE"; then
    fail "obsolete runtime path aliases remain"
fi

# Runtime third-party paths must have one source of truth.
broker_orender="$(sed -n 's/^#define DEFAULT_ORENDER "\(.*\)"$/\1/p' "$LIVE")"
broker_harletty="$(sed -n 's/^#define DEFAULT_HARLETTY "\(.*\)"$/\1/p' "$LIVE")"
broker_layout="$(sed -n 's/^#define DEFAULT_LAYOUT "\(.*\)"$/\1/p' "$LIVE")"
broker_post="$(sed -n 's/^#define DEFAULT_POSTPROCESS "\(.*\)"$/\1/p' "$LIVE")"
[ "$broker_orender" = "$(env_value AURORA_ORENDER_BIN)" ] || fail "Omniphony fallback drift"
[ "$broker_harletty" = "$(env_value AURORA_HARLETTY_BRIDGE)" ] || fail "Harletty fallback drift"
[ "$broker_layout" = "$(env_value AURORA_7_1_4_LAYOUT)" ] || fail "layout fallback drift"
[ "$broker_post" = "$(env_value AURORA_POSTPROCESS_BIN)" ] || fail "postprocessor fallback drift"

for install_line in \
    'install -m 0755 "$STAGE/bin/aurora-source-manager" "$ROOTFS/usr/local/sbin/aurora-source-manager"' \
    'install -m 0755 "$STAGE/bin/aurora-source-gate" "$ROOTFS/usr/local/sbin/aurora-source-gate"' \
    'install -m 0755 "$STAGE/bin/aurora-live-ingest" "$ROOTFS/usr/local/sbin/aurora-live-ingest"' \
    'install -m 0755 "$STAGE/bin/aurora-s6-postprocess" "$ROOTFS/usr/local/bin/aurora-s6-postprocess"' \
    'install -m 0755 "$STAGE/bin/aurora-ffs-daemon" "$ROOTFS/usr/local/sbin/aurora-ffs-daemon"'
 do
    [ "$(count_fixed "$install_line" "$ASSEMBLE")" -eq 1 ] || \
        fail "rootfs install missing/duplicated: $install_line"
 done
for service_name in aurora-ffs aurora-source-manager aurora-source-gate aurora-live-ingest; do
    [ "$(count_fixed "ln -sf /etc/init.d/$service_name \"\$ROOTFS/etc/runlevels/default/$service_name\"" "$ASSEMBLE")" -eq 1 ] || \
        fail "$service_name not enabled exactly once"
done

# One renderer, one postprocessor and shared clock-control primitives.
[ "$(count_fixed 'execl(orender, orender,' "$LIVE")" -eq 1 ] || fail "Omniphony exec path duplicated/missing"
[ "$(count_fixed 'execl(postprocess, postprocess,' "$LIVE")" -eq 1 ] || fail "postprocessor exec path duplicated/missing"
[ "$(count_fixed 'platform/s6/source-manager/aurora-source-manager.c' "$BUILD")" -eq 1 ] || fail "source manager build path duplicated/missing"
[ "$(count_fixed 'platform/s6/source-manager/aurora-source-gate.c' "$BUILD")" -eq 1 ] || fail "source gate build path duplicated/missing"
if grep -R -i -q -- 'camilladsp' "$INIT_DIR"; then
    fail "CamillaDSP must not run in parallel on S6"
fi
grep -Fq -- 'AsynchronousResampler, DriftController, DriftControllerConfig, RubatoAsrc' "$POST" || \
    fail "postprocessor is not using shared realtime clock primitives"
if grep -Eq -- '^[[:space:]]*(pub[[:space:]]+)?struct[[:space:]]+(RubatoAsrc|DriftController)[[:space:]]*\{' "$POST"; then
    fail "postprocessor redefines ASRC/drift primitive"
fi

# Pinned versions and realtime shape must agree.
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

echo "audit-runtime-wiring: PASS single hardware manifest, exclusive HDMI/local final mux, CONFIG-ACK ownership gate, no FunctionFS bypass, single renderer/DSP/ASRC chain, canonical runtime config"
