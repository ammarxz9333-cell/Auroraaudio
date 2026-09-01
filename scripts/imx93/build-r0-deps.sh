#!/usr/bin/env bash
set -euo pipefail

HARLETTY_COMMIT=4ccedec804de3b29c02fb2a69575c2f49bf2fb37
OMNIPHONY_COMMIT=44acc87a9cbf4b5ac8f474f51d87851d2c642550
DEPS_ROOT="${AURORA_DEPS_ROOT:-$HOME/aurora-deps}"

for cmd in git cargo rustc pkg-config; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
if ! pkg-config --exists libpipewire-0.3; then
  echo 'missing PipeWire development package (pkg-config: libpipewire-0.3)' >&2
  echo 'Debian/Ubuntu: sudo apt install libpipewire-0.3-dev pkg-config' >&2
  exit 2
fi

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
  local actual
  actual="$(git -C "$dir" rev-parse HEAD)"
  [[ "$actual" == "$commit" ]] || {
    echo "pin mismatch in $dir: expected $commit got $actual" >&2
    exit 3
  }
}

# Harletty's bridge has path dependencies on a sibling checkout named Omniphony,
# so keep these two repositories as siblings under DEPS_ROOT.
checkout_repo https://github.com/mgth/Omniphony.git \
  "$DEPS_ROOT/Omniphony" "$OMNIPHONY_COMMIT"
checkout_repo https://github.com/harletty/harletty-bridge.git \
  "$DEPS_ROOT/harletty-bridge" "$HARLETTY_COMMIT"

(
  cd "$DEPS_ROOT/harletty-bridge"
  cargo build --release -p harletty
  ./build_bridge.sh
)

(
  cd "$DEPS_ROOT/Omniphony/omniphony-renderer"
  cargo build --release --features pipewire
)

HARLETTY_CLI="$DEPS_ROOT/harletty-bridge/target/release/harletty"
HARLETTY_BRIDGE="$DEPS_ROOT/harletty-bridge/target/release/libharletty_bridge.so"
ORENDER="$DEPS_ROOT/Omniphony/omniphony-renderer/target/release/orender"

for artifact in "$HARLETTY_CLI" "$HARLETTY_BRIDGE" "$ORENDER"; do
  [[ -e "$artifact" ]] || { echo "expected build artifact missing: $artifact" >&2; exit 4; }
done

cat <<EOF
R0 dependencies built and pin-verified.
Harletty CLI:    $HARLETTY_CLI
Harletty bridge: $HARLETTY_BRIDGE
Omniphony:       $ORENDER
Pinned commits:  Harletty=$HARLETTY_COMMIT Omniphony=$OMNIPHONY_COMMIT
EOF
