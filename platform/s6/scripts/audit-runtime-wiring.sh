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
PINMUX_PROOF="$ROOT/docs/AURORA_REALTIME_MCU_PINMUX_PROOF.md"
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

# Hardware selection has exactly one machine-readable owner. Runtime/CI/docs
# consume symbolic AURORA_REALTIME_MCU_* fields instead of duplicating a part
# number. A legacy source-directory name is explicitly not target truth.
[ -f "$HW_TARGET" ] || fail "hardware target manifest missing"
# shellcheck disable=SC1090
. "$HW_TARGET"
[ "$AURORA_REALTIME_MCU_ROLE" = "aurora-realtime-mcu" ] || \
    fail "unexpected realtime MCU symbolic role"
[ -n "$AURORA_REALTIME_MCU_PART" ] || fail "realtime MCU part is empty"
[ -n "$AURORA_REALTIME_MCU_PACKAGE" ] || fail "realtime MCU package is empty"
[ -d "$ROOT/$AURORA_REALTIME_MCU_SOURCE_DIR/include" ] || \
    fail "realtime MCU include directory does not exist"
[ -d "$ROOT/$AURORA_REALTIME_MCU_SOURCE_DIR/src" ] || \
    fail "realtime MCU source directory does not exist"
[ -d "$ROOT/$AURORA_REALTIME_MCU_SOURCE_DIR/test" ] || \
    fail "realtime MCU test directory does not exist"
[ "$AURORA_REALTIME_MCU_USB_HS_HOST" = 1 ] || fail "target lacks mandatory USB HS host"
[ "$AURORA_REALTIME_MCU_USB_HS_PHY" = ULPI ] || fail "target HS PHY contract is not ULPI"
[ "$AURORA_REALTIME_MCU_ULPI_DIRECT_PINS" = 1 ] || fail "target does not use direct ULPI pins"
[ "$AURORA_REALTIME_MCU_SAI_RX" = 1 ] || fail "target lacks mandatory serial-audio RX"
[ "$AURORA_REALTIME_MCU_SAI_TDM_TX" = 1 ] || fail "target lacks mandatory TDM TX"
[ "$AURORA_REALTIME_MCU_DMA" = 1 ] || fail "target lacks mandatory DMA"
[ "$AURORA_REALTIME_MCU_PINMUX_STATUS" = "datasheet_verified" ] || \
    fail "realtime MCU pinmux is not datasheet-verified"
[ -n "$AURORA_REALTIME_MCU_PINMUX_PROOF" ] || fail "pinmux proof identifier missing"
[ -f "$PINMUX_PROOF" ] || fail "pinmux proof document missing"

# No physical MCU pin may own two simultaneous roles. Read the manifest itself,
# not `env`: sourced shell assignments are not necessarily exported.
duplicate_pins="$(sed -n 's/^AURORA_PIN_[^=]*=//p' "$HW_TARGET" | sort | uniq -d)"
[ -z "$duplicate_pins" ] || fail "duplicate realtime-MCU pin assignments: $duplicate_pins"

# The active concrete part string must not leak into runtime-critical consumers;
# otherwise a future part swap would again require editing multiple files.
for file in \
    "$LIVE" \
    "$POST" \
    "$BUILD" \
    "$ASSEMBLE" \
    "$SERVICE" \
    "$FFS_SERVICE" \
    "$ROOT/.github/workflows/s6-appliance-ci.yml" \
    "$ROOT/docs/AURORA_USB_S6_STM32_PROTOCOL.md" \
    "$ROOT/docs/AURORA_EARC_STM32_PHYSICAL_BRINGUP.md" \
    "$ROOT/docs/AURORA_SYSTEM_ARCHITECTURE_HARDENING_CONTRACT.md" \
    "$PINMUX_PROOF"
do
    if grep -Fq -- "$AURORA_REALTIME_MCU_PART" "$file"; then
        fail "concrete realtime MCU part leaked outside hardware target manifest: $file"
    fi
done

# Exactly two Aurora services belong in the S6 appliance: the FunctionFS
# transport bridge and the single live audio pipeline. Adding another Aurora
# audio daemon must be an explicit architecture change, never an accidental
# second processing path.
set -- "$INIT_DIR"/aurora-*
[ "$#" -eq 2 ] || fail "expected exactly two Aurora init services, found $#"
[ -f "$FFS_SERVICE" ] || fail "aurora-ffs service missing"
[ -f "$SERVICE" ] || fail "aurora-live-ingest service missing"
sh -n "$FFS_SERVICE" || fail "aurora-ffs service has shell syntax errors"
sh -n "$SERVICE" || fail "aurora-live-ingest service has shell syntax errors"
[ "$(count_fixed 'need aurora-ffs' "$SERVICE")" -eq 1 ] || \
    fail "live ingest must depend on exactly one FunctionFS bridge"

# Runtime path ownership is canonical: aurora.env owns all paths and the
# OpenRC service consumes that manifest directly. Old alias keys are forbidden
# because they can silently drift from the variables the broker actually reads.
[ "$(count_fixed '. /etc/aurora/aurora.env' "$SERVICE")" -eq 1 ] || \
    fail "live service must source exactly one runtime manifest"
[ "$(count_fixed 'command="$AURORA_LIVE_INGEST_BIN"' "$SERVICE")" -eq 1 ] || \
    fail "live service command must come from AURORA_LIVE_INGEST_BIN"
[ "$(count_fixed 'AURORA_CONFIG_MISSING=1' "$SERVICE")" -eq 1 ] || \
    fail "live service must record a missing runtime manifest"
[ "$(count_fixed '[ "$AURORA_CONFIG_MISSING" -eq 0 ] || return 1' "$SERVICE")" -eq 1 ] || \
    fail "live service must fail closed when the runtime manifest is missing"
for assignment in \
    'AURORA_LIVE_INGEST_BIN=/usr/local/sbin/aurora-live-ingest' \
    'AURORA_POSTPROCESS_BIN=/usr/local/bin/aurora-s6-postprocess' \
    'AURORA_ORENDER_BIN=/opt/aurora/external/orender' \
    'AURORA_HARLETTY_BRIDGE=/opt/aurora/external/libharletty_bridge.so' \
    'AURORA_7_1_4_LAYOUT=/etc/aurora/layouts/7.1.4.yaml'
do
    [ "$(count_fixed "$assignment" "$ENVFILE")" -eq 1 ] || \
        fail "runtime manifest assignment missing or duplicated: $assignment"
done
if grep -Eq '^(HARLETTY_BIN|OMNIPHONY_BIN)=' "$ENVFILE"; then
    fail "obsolete Harletty/Omniphony path aliases remain in runtime manifest"
fi
for variable in AURORA_LIVE_INGEST_BIN AURORA_POSTPROCESS_BIN AURORA_ORENDER_BIN AURORA_HARLETTY_BRIDGE AURORA_7_1_4_LAYOUT; do
    [ "$(count_fixed "\$$variable" "$SERVICE")" -ge 1 ] || \
        fail "service does not consume canonical runtime variable $variable"
done

# The broker retains compile-time fallbacks for standalone tests/manual runs,
# but the appliance service cannot reach them when aurora.env is missing. Lock
# every fallback to the canonical manifest so they can never become a second
# drifting runtime configuration.
broker_orender="$(sed -n 's/^#define DEFAULT_ORENDER "\(.*\)"$/\1/p' "$LIVE")"
broker_harletty="$(sed -n 's/^#define DEFAULT_HARLETTY "\(.*\)"$/\1/p' "$LIVE")"
broker_layout="$(sed -n 's/^#define DEFAULT_LAYOUT "\(.*\)"$/\1/p' "$LIVE")"
broker_post="$(sed -n 's/^#define DEFAULT_POSTPROCESS "\(.*\)"$/\1/p' "$LIVE")"
[ "$broker_orender" = "$(env_value AURORA_ORENDER_BIN)" ] || \
    fail "broker Omniphony fallback drifted from canonical manifest"
[ "$broker_harletty" = "$(env_value AURORA_HARLETTY_BRIDGE)" ] || \
    fail "broker Harletty fallback drifted from canonical manifest"
[ "$broker_layout" = "$(env_value AURORA_7_1_4_LAYOUT)" ] || \
    fail "broker layout fallback drifted from canonical manifest"
[ "$broker_post" = "$(env_value AURORA_POSTPROCESS_BIN)" ] || \
    fail "broker postprocessor fallback drifted from canonical manifest"

# Every service/runtime path must resolve to exactly one file installed by the
# rootfs assembler. This prevents a green component build from being wired to a
# different binary or library in the appliance image.
for install_line in \
    'install -m 0755 "$STAGE/bin/aurora-live-ingest" "$ROOTFS/usr/local/sbin/aurora-live-ingest"' \
    'install -m 0755 "$STAGE/bin/aurora-s6-postprocess" "$ROOTFS/usr/local/bin/aurora-s6-postprocess"' \
    'install -m 0755 "$STAGE/bin/aurora-ffs-daemon" "$ROOTFS/usr/local/sbin/aurora-ffs-daemon"' \
    'install -m 0755 "$STAGE/bin/orender" "$ROOTFS/opt/aurora/external/orender"' \
    'install -m 0755 "$STAGE/lib/libharletty_bridge.so" "$ROOTFS/opt/aurora/external/libharletty_bridge.so"' \
    'install -m 0644 "$STAGE/share/omniphony/layouts/7.1.4.yaml" "$ROOTFS/etc/aurora/layouts/7.1.4.yaml'
do
    [ "$(count_fixed "$install_line" "$ASSEMBLE")" -eq 1 ] || \
        fail "rootfs install missing or duplicated: $install_line"
done
[ "$(count_fixed 'command="/usr/local/sbin/aurora-ffs-daemon"' "$FFS_SERVICE")" -eq 1 ] || \
    fail "FunctionFS service is not wired to the installed daemon"
[ "$(count_fixed 'ln -sf /etc/init.d/aurora-ffs "$ROOTFS/etc/runlevels/default/aurora-ffs"' "$ASSEMBLE")" -eq 1 ] || \
    fail "FunctionFS service is not enabled exactly once"
[ "$(count_fixed 'ln -sf /etc/init.d/aurora-live-ingest "$ROOTFS/etc/runlevels/default/aurora-live-ingest"' "$ASSEMBLE")" -eq 1 ] || \
    fail "live audio service is not enabled exactly once"

# The S6 build intentionally excludes the workspace default CamillaDSP and
# simulation features. Only the dedicated realtime binary belongs on this path.
grep -Fq -- 'cargo build --locked --release -j "$JOBS" -p aurora-cli --no-default-features --features realtime' "$BUILD" || \
    fail "S6 Aurora build is not restricted to realtime/no-default-features"
if grep -R -i -q -- 'camilladsp' "$INIT_DIR"; then
    fail "CamillaDSP appears in an enabled S6 init service"
fi

# One renderer process followed by one postprocessor process. More than one of
# either is a real double-render/double-DSP defect.
[ "$(count_fixed 'execl(orender, orender,' "$LIVE")" -eq 1 ] || \
    fail "expected exactly one Omniphony exec path"
[ "$(count_fixed 'execl(postprocess, postprocess,' "$LIVE")" -eq 1 ] || \
    fail "expected exactly one postprocessor exec path"
[ "$(count_fixed 'install -m 0755 target/release/aurora-s6-postprocess "$OUT/bin/aurora-s6-postprocess"' "$BUILD")" -eq 1 ] || \
    fail "postprocessor staging path mismatch"

# Clock correction must reuse the existing Aurora realtime primitives. A local
# second ASRC or PI controller in the S6 binary would be duplicated ownership.
grep -Fq -- 'AsynchronousResampler, DriftController, DriftControllerConfig, RubatoAsrc' "$POST" || \
    fail "S6 postprocessor is not using Aurora realtime ASRC/drift primitives"
if grep -Eq -- '^[[:space:]]*(pub[[:space:]]+)?struct[[:space:]]+(RubatoAsrc|DriftController)[[:space:]]*\{' "$POST"; then
    fail "S6 postprocessor redefines an existing ASRC/drift primitive"
fi

# Version truth must agree across the runtime manifest and native build defaults.
env_harletty="$(sed -n 's/^HARLETTY_VERSION=//p' "$ENVFILE")"
build_harletty="$(sed -n 's/^HARLETTY_VERSION="${HARLETTY_VERSION:-\(.*\)}"$/\1/p' "$BUILD")"
[ -n "$env_harletty" ] && [ "$env_harletty" = "$build_harletty" ] || \
    fail "Harletty version drift: env=$env_harletty build=$build_harletty"

env_omniphony="$(sed -n 's/^OMNIPHONY_VERSION=//p' "$ENVFILE")"
build_omniphony="$(sed -n 's/^OMNIPHONY_VERSION="${OMNIPHONY_VERSION:-\(.*\)}"$/\1/p' "$BUILD")"
[ -n "$env_omniphony" ] && [ "$env_omniphony" = "$build_omniphony" ] || \
    fail "Omniphony version drift: env=$env_omniphony build=$build_omniphony"

env_navidrome="$(sed -n 's/^NAVIDROME_VERSION=//p' "$ENVFILE")"
build_navidrome="$(sed -n 's/^NAVIDROME_VERSION="${NAVIDROME_VERSION:-\(.*\)}"$/\1/p' "$BUILD")"
[ -n "$env_navidrome" ] && [ "$env_navidrome" = "$build_navidrome" ] || \
    fail "Navidrome version drift: env=$env_navidrome build=$build_navidrome"

# Core stream shape must remain one canonical contract end-to-end, including
# the selected hardware target's declared transport shape.
grep -Fq -- 'AURORA_SAMPLE_RATE=48000' "$ENVFILE" || fail "runtime sample rate is not 48 kHz"
grep -Fq -- 'AURORA_BLOCK_FRAMES=40' "$ENVFILE" || fail "runtime block size is not 40 frames"
grep -Fq -- 'AURORA_LAYOUT=7.1.4' "$ENVFILE" || fail "runtime layout is not 7.1.4"
[ "$AURORA_REALTIME_MCU_SAMPLE_RATE" = 48000 ] || fail "hardware target sample rate drift"
[ "$AURORA_REALTIME_MCU_CHANNELS" = 12 ] || fail "hardware target channel-count drift"
[ "$AURORA_REALTIME_MCU_PERIOD_FRAMES" = 40 ] || fail "hardware target period drift"

echo "audit-runtime-wiring: PASS single service chain, single DSP path, single symbolic realtime-MCU target, unique datasheet-verified pin map, canonical appliance config, exact rootfs wiring, shared clock primitives, consistent versions"
