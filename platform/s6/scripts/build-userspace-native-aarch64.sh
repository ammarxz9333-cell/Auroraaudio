#!/bin/sh
set -eu

# Build AuroraOS-S6 userspace on a native Alpine AArch64 builder.
# This deliberately does not flash a phone or modify a kernel.

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
OUT="${AURORA_OUT:-$ROOT/out/s6-aarch64}"
WORK="${AURORA_WORK:-$ROOT/.work/s6-aarch64}"
JOBS="${JOBS:-4}"
CC="${CC:-cc}"
HARLETTY_VERSION="${HARLETTY_VERSION:-v0.7.4}"
OMNIPHONY_VERSION="${OMNIPHONY_VERSION:-v0.5.2}"
NAVIDROME_VERSION="${NAVIDROME_VERSION:-0.63.2}"
NAVIDROME_SHA256="5b74fb0eea5d48e3eb7565ea4116284232509e94431cb3756aaac2128dd50a43"

fail() { echo "build-userspace-native-aarch64: $*" >&2; exit 1; }
[ "$(uname -m)" = "aarch64" ] || fail "native aarch64 builder required"
[ -f /etc/alpine-release ] || fail "Alpine Linux builder required"

for cmd in cargo rustc git cmake pkg-config curl sha256sum tar "$CC"; do
    command -v "$cmd" >/dev/null 2>&1 || fail "missing build dependency: $cmd"
done

mkdir -p "$OUT/bin" "$OUT/lib" "$OUT/share/omniphony/layouts" "$OUT/navidrome" "$WORK"

# Validate the isolated USB wire-format crate without touching the main
# workspace lockfile. It has no external dependencies and carries its own lock.
(
    cd "$ROOT"
    cargo test --locked --manifest-path crates/aurora-usb-protocol/Cargo.toml
)

# Build the FunctionFS bridge natively for AuroraOS-S6. The streaming parser is
# shared with the STM32 firmware core so split/coalesced bulk reads follow one
# implementation on both peers.
"$CC" -D_GNU_SOURCE -std=c11 -O2 -Wall -Wextra -Werror \
    -I"$ROOT/protocol" \
    "$ROOT/platform/s6/usb-gadget/aurora-ffs-daemon.c" \
    "$ROOT/protocol/aurora_usb_stream_v1.c" \
    -o "$OUT/bin/aurora-ffs-daemon"

# Live immersive streaming broker. It deliberately does NOT decode or unwrap
# IEC61937 itself. Complete encoded frames received from STM32 are forwarded as
# a byte stream to Omniphony stdin; Omniphony v0.5.2 owns the streaming
# IEC61937 parser and passes typed packets to the Harletty bridge. Rendered
# 7.1.4 raw-f32 is converted to Aurora protocol PCM_S32LE periods for STM32.
"$CC" -std=c11 -O2 -Wall -Wextra -Werror \
    -I"$ROOT/protocol" \
    "$ROOT/platform/s6/live-ingest/aurora-live-ingest.c" \
    -lm -o "$OUT/bin/aurora-live-ingest"

# Aurora-owned Rust baseline. Exclude simulation and the external CamillaDSP process
# from the appliance binary; realtime CPAL remains available for bring-up/testing.
(
    cd "$ROOT"
    cargo build --locked --release -j "$JOBS" -p aurora-cli --no-default-features --features realtime
    install -m 0755 target/release/aurora-cli "$OUT/bin/aurora-cli"
)

# Omniphony and Harletty must be sibling checkouts because the Harletty bridge
# intentionally consumes Omniphony's bridge ABI through path dependencies.
if [ ! -d "$WORK/Omniphony/.git" ]; then
    git clone https://github.com/mgth/Omniphony.git "$WORK/Omniphony"
fi
(
    cd "$WORK/Omniphony"
    git fetch --tags --force
    git checkout --detach "$OMNIPHONY_VERSION"
    cd omniphony-renderer
    cargo build --release -j "$JOBS" -p omniphony-renderer
    install -m 0755 target/release/orender "$OUT/bin/orender"
    install -m 0644 ../layouts/7.1.4.yaml "$OUT/share/omniphony/layouts/7.1.4.yaml"
)

if [ ! -d "$WORK/harletty-bridge/.git" ]; then
    git clone https://github.com/harletty/harletty-bridge.git "$WORK/harletty-bridge"
fi
(
    cd "$WORK/harletty-bridge"
    git fetch --tags --force
    git checkout --detach "$HARLETTY_VERSION"
    cargo build --release -j "$JOBS" -p harletty-bridge
    install -m 0755 target/release/libharletty_bridge.so "$OUT/lib/libharletty_bridge.so"
)

# Navidrome publishes a native Linux ARM64 tarball. Verify the release digest
# before it is admitted to the appliance staging tree.
NAV_ARCHIVE="$WORK/navidrome_${NAVIDROME_VERSION}_linux_arm64.tar.gz"
if [ ! -f "$NAV_ARCHIVE" ]; then
    curl -fL "https://github.com/navidrome/navidrome/releases/download/v${NAVIDROME_VERSION}/navidrome_${NAVIDROME_VERSION}_linux_arm64.tar.gz" -o "$NAV_ARCHIVE"
fi
echo "$NAVIDROME_SHA256  $NAV_ARCHIVE" | sha256sum -c -
tar -xzf "$NAV_ARCHIVE" -C "$OUT/navidrome"
[ -x "$OUT/navidrome/navidrome" ] || fail "Navidrome binary missing after extraction"

# Record exact provenance used by the bundle.
{
    echo "architecture=aarch64"
    echo "aurora_commit=$(git -C "$ROOT" rev-parse HEAD)"
    echo "aurora_usb_protocol=1"
    echo "live_streaming_ingest=iec61937-direct-to-omniphony"
    echo "harletty=$HARLETTY_VERSION"
    echo "omniphony=$OMNIPHONY_VERSION"
    echo "navidrome=v$NAVIDROME_VERSION"
    echo "navidrome_sha256=$NAVIDROME_SHA256"
} > "$OUT/BUILD-MANIFEST.txt"

sha256sum "$OUT/bin/aurora-ffs-daemon" > "$OUT/bin/aurora-ffs-daemon.sha256"
sha256sum "$OUT/bin/aurora-live-ingest" > "$OUT/bin/aurora-live-ingest.sha256"
echo "Userspace staging complete: $OUT"