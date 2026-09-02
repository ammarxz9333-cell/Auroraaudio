#!/usr/bin/env bash
set -euo pipefail

HARLETTY_COMMIT=4ccedec804de3b29c02fb2a69575c2f49bf2fb37
OMNIPHONY_COMMIT=44acc87a9cbf4b5ac8f474f51d87851d2c642550
DEPS_ROOT="${AURORA_DEPS_ROOT:-$HOME/aurora-deps}"
DEPS_PROFILE="${AURORA_DEPS_PROFILE:-release}"

case "$DEPS_PROFILE" in
  release)
    CARGO_PROFILE_ARGS=(--release)
    TARGET_PROFILE=release
    ;;
  debug)
    CARGO_PROFILE_ARGS=()
    TARGET_PROFILE=debug
    ;;
  *)
    echo "AURORA_DEPS_PROFILE must be 'release' or 'debug', got: $DEPS_PROFILE" >&2
    exit 2
    ;;
esac

for cmd in git cargo rustc pkg-config; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
pkg-config --exists alsa || {
  echo 'missing ALSA development package; Debian/Ubuntu: sudo apt install libasound2-dev pkg-config' >&2
  exit 2
}

mkdir -p "$DEPS_ROOT"
checkout_repo() {
  local url=$1 dir=$2 commit=$3
  if [[ ! -d "$dir/.git" ]]; then
    git clone "$url" "$dir"
  fi
  git -C "$dir" fetch --all --tags --prune
  git -C "$dir" checkout --detach "$commit"
  [[ "$(git -C "$dir" rev-parse HEAD)" == "$commit" ]] || exit 3
}

# Harletty expects the Omniphony checkout as a sibling path dependency.
checkout_repo https://github.com/mgth/Omniphony.git "$DEPS_ROOT/Omniphony" "$OMNIPHONY_COMMIT"
checkout_repo https://github.com/harletty/harletty-bridge.git "$DEPS_ROOT/harletty-bridge" "$HARLETTY_COMMIT"

# R2 never runs the offline Harletty CLI. Build only the decoder bridge that
# liborender loads at runtime. Production defaults to release; CI may request a
# debug build to prove functionality without spending time on LLVM optimisation.
(
  cd "$DEPS_ROOT/harletty-bridge"
  cargo build "${CARGO_PROFILE_ARGS[@]}" -p harletty-bridge
)

# R2 embeds the headless engine through its stable C ABI. Do NOT build orender
# CLI or PipeWire. --no-default-features also removes the desktop SOFA feature.
(
  cd "$DEPS_ROOT/Omniphony/omniphony-renderer"
  cargo build "${CARGO_PROFILE_ARGS[@]}" -p orender_ffi --no-default-features
)

BRIDGE="$DEPS_ROOT/harletty-bridge/target/$TARGET_PROFILE/libharletty_bridge.so"
LIBORENDER="$DEPS_ROOT/Omniphony/omniphony-renderer/target/$TARGET_PROFILE/liborender.so"
for artifact in "$BRIDGE" "$LIBORENDER"; do
  [[ -e "$artifact" ]] || { echo "expected artifact missing: $artifact" >&2; exit 4; }
done

printf 'Aurora R2 dependencies ready (%s).\nHarletty bridge: %s\nliborender: %s\nPins: Harletty=%s Omniphony=%s\n' \
  "$TARGET_PROFILE" "$BRIDGE" "$LIBORENDER" "$HARLETTY_COMMIT" "$OMNIPHONY_COMMIT"
