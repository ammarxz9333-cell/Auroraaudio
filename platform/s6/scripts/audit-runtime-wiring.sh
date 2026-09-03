#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
INIT_DIR="$ROOT/platform/s6/rootfs/etc/init.d"
LIVE="$ROOT/platform/s6/live-ingest/aurora-live-ingest.c"
POST="$ROOT/crates/aurora-cli/src/bin/aurora-s6-postprocess.rs"
BUILD="$ROOT/platform/s6/scripts/build-userspace-native-aarch64.sh"
ASSEMBLE="$ROOT/platform/s6/scripts/assemble-rootfs-native-aarch64.sh"
ENVFILE="$ROOT/platform/s6/rootfs/etc/aurora/aurora.env"
SERVICE="$INIT_DIR/aurora-live-ingest"

fail() {
    echo "audit-runtime-wiring: FAIL: $*" >&2
    exit 1
}

count_fixed() {
    pattern="$1"
    file="$2"
    grep -F -c -- "$pattern" "$file" || true
}

# Exactly two Aurora services belong in the S6 appliance: the FunctionFS
# transport bridge and the single live audio pipeline. Adding another Aurora
# audio daemon must be an explicit architecture change, never an accidental
# second processing path.
set -- "$INIT_DIR"/aurora-*
[ "$#" -eq 2 ] || fail "expected exactly two Aurora init services, found $#"
[ -f "$INIT_DIR/aurora-ffs" ] || fail "aurora-ffs service missing"
[ -f "$INIT_DIR/aurora-live-ingest" ] || fail "aurora-live-ingest service missing"

[ "$(count_fixed 'command="/usr/local/sbin/aurora-live-ingest"' "$SERVICE")" -eq 1 ] || \
    fail "live ingest service command is missing or duplicated"
[ "$(count_fixed 'need aurora-ffs' "$SERVICE")" -eq 1 ] || \
    fail "live ingest must depend on exactly one FunctionFS bridge"

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
[ "$(count_fixed '#define DEFAULT_POSTPROCESS "/usr/local/bin/aurora-s6-postprocess"' "$LIVE")" -eq 1 ] || \
    fail "broker postprocessor path is missing or duplicated"
[ "$(count_fixed 'AURORA_POSTPROCESS_BIN=/usr/local/bin/aurora-s6-postprocess' "$ENVFILE")" -eq 1 ] || \
    fail "runtime manifest postprocessor path mismatch"
[ "$(count_fixed 'install -m 0755 target/release/aurora-s6-postprocess "$OUT/bin/aurora-s6-postprocess"' "$BUILD")" -eq 1 ] || \
    fail "postprocessor staging path mismatch"
[ "$(count_fixed 'install -m 0755 "$STAGE/bin/aurora-s6-postprocess" "$ROOTFS/usr/local/bin/aurora-s6-postprocess"' "$ASSEMBLE")" -eq 1 ] || \
    fail "postprocessor rootfs install path mismatch"

# Clock correction must reuse the existing Aurora realtime primitives. A local
# second ASRC or PI controller in the S6 binary would be duplicated ownership.
grep -Fq -- 'AsynchronousResampler, DriftController, DriftControllerConfig, RubatoAsrc' "$POST" || \
    fail "S6 postprocessor is not using Aurora realtime ASRC/drift primitives"
if grep -Eq -- '^[[:space:]]*(pub[[:space:]]+)?struct[[:space:]]+(RubatoAsrc|DriftController)[[:space:]]*\{' "$POST"; then
    fail "S6 postprocessor redefines an existing ASRC/drift primitive"
fi

# Version truth must have one source value across runtime manifest and native
# build defaults. This catches silent build/runtime version drift.
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

# Core stream shape must remain one canonical contract end-to-end.
grep -Fq -- 'AURORA_SAMPLE_RATE=48000' "$ENVFILE" || fail "runtime sample rate is not 48 kHz"
grep -Fq -- 'AURORA_BLOCK_FRAMES=40' "$ENVFILE" || fail "runtime block size is not 40 frames"
grep -Fq -- 'AURORA_LAYOUT=7.1.4' "$ENVFILE" || fail "runtime layout is not 7.1.4"

echo "audit-runtime-wiring: PASS single service chain, single DSP path, shared clock primitives, consistent versions"
