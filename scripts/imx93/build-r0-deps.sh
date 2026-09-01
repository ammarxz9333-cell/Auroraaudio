#!/usr/bin/env bash
set -euo pipefail

HARLETTY_COMMIT=4ccedec804de3b29c02fb2a69575c2f49bf2fb37
OMNIPHONy_COMMIT=44acc87a9cbf4b5ac8f474f51d87851d2c642550
DEPS_ROOT="${AURORA_DEPS_ROOT:-$HOME/aurora-deps}"

mkdir -p "$DEPS_ROOT"

checkout_repo() {
  local url=$1
  local dir=$2
  local commit=$3
  if [[ ! -d "$dir/.git" ]]; then
    git clone "$url" "$dir"
  fi
  git -C "$dir" fetch --all --tags --prune
  git -C "$dir" checkout --detach "$commit"
}

checkout_repo https://github.com/harletty/harletty-bridge.git \
  "$DEPS_ROOT/harletty-bridge" "$HARLETTY_COMMIT"
checkout_repo https://github.com/mgth/Omniphony.git \
  "$DEPS_ROOT/Omniphony" "$OMNIPHONy_COMMIT"

(
  cd "$DEPS_ROOT/harletty-bridge"
  cargo build --release -p harletty
  ./build_bridge.sh
)

(
  cd "$DEPS_ROOT/Omniphony/omniphony-renderer"
  cargo build --release --features pipewire
)

cat <<EOF
R0 dependencies built.
Harletty CLI:    $DEPS_ROOT/harletty-bridge/target/release/harletty
Harletty bridge: $DEPS_ROOT/harletty-bridge/target/release/libharletty_bridge.so
Omniphony root:  $DEPS_ROOT/Omniphony
Pinned commits:  Harletty=$HARLETTY_COMMIT Omniphony=$OMNIPHONy_COMMIT
EOF
