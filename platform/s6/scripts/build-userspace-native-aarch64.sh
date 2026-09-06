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
HARLETTY_COMMIT="10943821cca7e6886c11f45d2267b06d76e6db7c"
OMNIPHONY_VERSION="${OMNIPHONY_VERSION:-v0.5.2}"
OMNIPHONY_COMMIT="f9a79721af64ad9c39042d4deded158b568fc598"
OMNIPHONY_PATCH="$ROOT/platform/s6/patches/omniphony-v0.5.2-low-latency-stdout.patch"
NAVIDROME_VERSION="${NAVIDROME_VERSION:-0.63.2}"
NAVIDROME_SHA256="5b74fb0eea5d48e3eb7565ea4116284232509e94431cb3756aaac2128dd50a43"

fail() { echo "build-userspace-native-aarch64: $*" >&2; exit 1; }
[ "$(uname -m)" = "aarch64" ] || fail "native aarch64 builder required"
[ -f /etc/alpine-release ] || fail "Alpine Linux builder required"
[ "$HARLETTY_VERSION" = "v0.7.4" ] || fail "Harletty adapter is pinned to v0.7.4"
[ "$OMNIPHONY_VERSION" = "v0.5.2" ] || fail "Omniphony adapter is pinned to v0.5.2"

for cmd in cargo rustc git cmake pkg-config curl sha256sum tar "$CC"; do
    command -v "$cmd" >/dev/null 2>&1 || fail "missing build dependency: $cmd"
done

mkdir -p "$OUT/bin" "$OUT/lib" "$OUT/share/omniphony/layouts" "$OUT/navidrome" "$WORK"

(
    cd "$ROOT"
    cargo test --locked --manifest-path crates/aurora-usb-protocol/Cargo.toml
)

"$CC" -D_GNU_SOURCE -std=c11 -O2 -Wall -Wextra -Werror \
    -I"$ROOT/protocol" \
    "$ROOT/platform/s6/usb-gadget/aurora-ffs-daemon.c" \
    "$ROOT/protocol/aurora_usb_stream_v1.c" \
    -o "$OUT/bin/aurora-ffs-daemon"

# Source arbitration is a separate control plane. The HDMI source gate is the
# only process allowed to bridge live-ingest PCM into the FunctionFS backend;
# it keeps CONFIG/clock traffic flowing but ramps audio to silence unless the
# source manager grants HDMI/eARC ownership.
"$CC" -std=c11 -O2 -Wall -Wextra -Werror \
    -I"$ROOT/protocol" \
    "$ROOT/platform/s6/source-manager/aurora-source-manager.c" \
    -o "$OUT/bin/aurora-source-manager"

"$CC" -std=c11 -O2 -Wall -Wextra -Werror \
    -I"$ROOT/protocol" \
    "$ROOT/platform/s6/source-manager/aurora-source-gate.c" \
    -lm -o "$OUT/bin/aurora-source-gate"

"$CC" -std=c11 -O2 -Wall -Wextra -Werror \
    -I"$ROOT/protocol" \
    "$ROOT/platform/s6/source-manager/aurora-source-ctl.c" \
    -lm -o "$OUT/bin/aurora-source-ctl"

install -m 0755 "$ROOT/platform/s6/surround-upmix/aurora-surround-upmix.sh" \
    "$OUT/bin/aurora-surround-upmix"

# Live immersive streaming broker. It deliberately does NOT decode or unwrap
# IEC61937 itself. Complete encoded frames are forwarded as a byte stream to
# Omniphony stdin; Omniphony v0.5.2 owns the streaming IEC61937 parser and
# passes typed packets to the Harletty bridge. The runtime manifest routes this
# broker through aurora-source-gate rather than directly to FunctionFS.
"$CC" -std=c11 -O2 -Wall -Wextra -Werror \
    -I"$ROOT/protocol" \
    "$ROOT/platform/s6/live-ingest/aurora-live-ingest.c" \
    -lm -o "$OUT/bin/aurora-live-ingest"

(
    cd "$ROOT"
    cargo build --locked --release -j "$JOBS" -p aurora-cli --no-default-features --features realtime
    install -m 0755 target/release/aurora-cli "$OUT/bin/aurora-cli"
    install -m 0755 target/release/aurora-s6-postprocess "$OUT/bin/aurora-s6-postprocess"
    install -m 0755 target/release/aurora-self-test "$OUT/bin/aurora-self-test"
)

if [ ! -d "$WORK/Omniphony/.git" ]; then
    git clone https://github.com/mgth/Omniphony.git "$WORK/Omniphony"
fi
(
    cd "$WORK/Omniphony"
    git fetch --tags --force
    git checkout --detach "$OMNIPHONY_VERSION"
    [ "$(git rev-parse HEAD)" = "$OMNIPHONY_COMMIT" ] || \
        fail "Omniphony tag does not match pinned source commit"
    git reset --hard "$OMNIPHONY_COMMIT"

    [ "$OMNIPHONY_VERSION" = "v0.5.2" ] || \
        fail "Omniphony low-latency patch is pinned to v0.5.2, got $OMNIPHONY_VERSION"
    [ -f "$OMNIPHONY_PATCH" ] || fail "missing Omniphony patch: $OMNIPHONY_PATCH"
    git apply --check "$OMNIPHONY_PATCH" || \
        fail "Omniphony low-latency patch no longer applies cleanly"
    git apply "$OMNIPHONY_PATCH"

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
    [ "$(git rev-parse HEAD)" = "$HARLETTY_COMMIT" ] || \
        fail "Harletty tag does not match pinned source commit"
    git diff --quiet HEAD -- || fail "Harletty checkout contains modified tracked source"
    cargo build --release -j "$JOBS" -p harletty-bridge
    install -m 0755 target/release/libharletty_bridge.so "$OUT/lib/libharletty_bridge.so"
)

NAV_ARCHIVE="$WORK/navidrome_${NAVIDROME_VERSION}_linux_arm64.tar.gz"
if [ ! -f "$NAV_ARCHIVE" ]; then
    curl -fL "https://github.com/navidrome/navidrome/releases/download/v${NAVIDROME_VERSION}/navidrome_${NAVIDROME_VERSION}_linux_arm64.tar.gz" -o "$NAV_ARCHIVE"
fi
echo "$NAVIDROME_SHA256  $NAV_ARCHIVE" | sha256sum -c -
tar -xzf "$NAV_ARCHIVE" -C "$OUT/navidrome"
[ -x "$OUT/navidrome/navidrome" ] || fail "Navidrome binary missing after extraction"

{
    echo "architecture=aarch64"
    echo "aurora_commit=$(git -C "$ROOT" rev-parse HEAD)"
    echo "aurora_usb_protocol=1"
    echo "aurora_usb_period_frames=40"
    echo "source_manager=priority-quiesce-watchdog-v1"
    echo "source_control_cli=status-mute-gain-lipsync-standby-v1"
    echo "hdmi_source_gate=managed-ramp-v1"
    echo "surround_upmix=ffmpeg-channel-bed-synthetic-heights-v1"
    echo "live_streaming_ingest=iec61937-omniphony-postprocess-source-gate"
    echo "postprocessor=aurora-s6-postprocess"
    echo "postprocessor_asrc=rubato-sinc-fixed-out"
    echo "postprocessor_bass_management=lr4-configurable"
    echo "postprocessor_limiter=linked-peak"
    echo "harletty=$HARLETTY_VERSION"
    echo "harletty_commit=$HARLETTY_COMMIT"
    echo "omniphony=$OMNIPHONY_VERSION"
    echo "omniphony_commit=$OMNIPHONY_COMMIT"
    echo "omniphony_patch=low-latency-raw-stdout-40-frames-v1"
    echo "navidrome=v$NAVIDROME_VERSION"
    echo "navidrome_sha256=$NAVIDROME_SHA256"
} > "$OUT/BUILD-MANIFEST.txt"

sha256sum "$OUT/bin/aurora-ffs-daemon" > "$OUT/bin/aurora-ffs-daemon.sha256"
sha256sum "$OUT/bin/aurora-source-manager" > "$OUT/bin/aurora-source-manager.sha256"
sha256sum "$OUT/bin/aurora-source-gate" > "$OUT/bin/aurora-source-gate.sha256"
sha256sum "$OUT/bin/aurora-source-ctl" > "$OUT/bin/aurora-source-ctl.sha256"
sha256sum "$OUT/bin/aurora-live-ingest" > "$OUT/bin/aurora-live-ingest.sha256"
sha256sum "$OUT/bin/aurora-s6-postprocess" > "$OUT/bin/aurora-s6-postprocess.sha256"
echo "Userspace staging complete: $OUT"

