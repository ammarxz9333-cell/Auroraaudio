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
grep -Fq 'const BUF_CAPACITY: usize = 40 * 12 * 4;' "$FILE"
! grep -Fq 'const BUF_CAPACITY: usize = 64 * 1024;' "$FILE"

# The file sink is std-only. Compile and run all of its unit tests so the pinned
# one-line latency patch cannot hide a Rust syntax/regression failure.
rustc --edition=2021 --test "$FILE" -o "$TMP/file-sink-tests"
"$TMP/file-sink-tests"

echo "Omniphony v0.5.2 12-channel/40-frame output buffer patch test passed"
