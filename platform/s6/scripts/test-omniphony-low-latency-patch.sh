#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
PATCH="$ROOT/platform/s6/patches/omniphony-v0.5.2-low-latency-stdout.patch"
VERSION="v0.5.2"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

[ -f "$PATCH" ] || { echo "missing patch: $PATCH" >&2; exit 1; }
command -v git >/dev/null 2>&1 || { echo "git required" >&2; exit 1; }
command -v rustc >/dev/null 2>&1 || { echo "rustc required" >&2; exit 1; }

git clone --quiet --depth 1 --branch "$VERSION" \
    https://github.com/mgth/Omniphony.git "$TMP/Omniphony"

cd "$TMP/Omniphony"
git apply --check "$PATCH"
git apply "$PATCH"

FILE="omniphony-renderer/audio_output/src/file_sink.rs"
grep -q 'LOW_LATENCY_STDOUT_FRAMES: usize = 256' "$FILE"
grep -q 'output_buffer_capacity(destination, format, channel_count)' "$FILE"

# file_sink.rs depends only on std, so compile its own unit tests directly. This
# avoids pulling the full PipeWire/dependency graph while still compiling the
# patched Rust and executing the 256-frame regression test.
rustc --edition=2021 --test "$FILE" -o "$TMP/file-sink-tests"
"$TMP/file-sink-tests" raw_stdout_buffer_is_one_256_frame_period --exact

echo "Omniphony v0.5.2 low-latency raw stdout patch test passed"
