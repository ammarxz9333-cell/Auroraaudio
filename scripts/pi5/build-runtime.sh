#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="$ROOT_DIR/config/external-components-v1.json"
OMNIP_ARM64_PATCH="$ROOT_DIR/validation/immersive/omniphony-v0.6.0-arm64-c-char.patch"
RUNTIME_ROOT="${AURORA_RUNTIME_ROOT:-$HOME/.local/share/aurora-runtime}"
SRC_DIR="$RUNTIME_ROOT/src"
BUILD_DIR="$RUNTIME_ROOT/build"
PREFIX="$RUNTIME_ROOT/install"
TOOLCHAIN="${AURORA_EXTERNAL_RUST_TOOLCHAIN:-stable}"

for cmd in git python3 cargo rustup; do
  command -v "$cmd" >/dev/null 2>&1 || {
    echo "missing required command: $cmd" >&2
    exit 2
  }
done

eval "$(python3 - "$MANIFEST" <<'PY'
import json, shlex, sys
doc=json.load(open(sys.argv[1], encoding="utf-8"))
c={x["id"]:x for x in doc["components"]}
for prefix, cid in (("OMNIP","omniphony"),("HARLETTY","harletty-bridge")):
    x=c[cid]
    print(f"{prefix}_UPSTREAM={shlex.quote(x['upstream'])}")
    print(f"{prefix}_COMMIT={shlex.quote(x['pinned_commit'])}")
    print(f"{prefix}_VERSION={shlex.quote(x['tested_version'])}")
PY
)"

mkdir -p "$SRC_DIR" "$BUILD_DIR" "$PREFIX/bin" "$PREFIX/lib" "$PREFIX/share/aurora"

checkout_exact() {
  local url="$1" commit="$2" dest="$3"
  if [[ ! -d "$dest/.git" ]]; then
    rm -rf "$dest"
    git init -q "$dest"
    git -C "$dest" remote add origin "$url"
  fi
  git -C "$dest" fetch -q --depth 1 origin "$commit"
  git -C "$dest" checkout -q --detach FETCH_HEAD
  git -C "$dest" reset -q --hard "$commit"
  git -C "$dest" clean -q -fdx
  local actual
  actual="$(git -C "$dest" rev-parse HEAD)"
  [[ "$actual" == "$commit" ]] || {
    echo "commit mismatch for $url: expected=$commit actual=$actual" >&2
    exit 1
  }
}

HAR_DIR="$SRC_DIR/harletty-bridge"
OMNI_DIR="$SRC_DIR/Omniphony"

echo "== Aurora Pi5 runtime: exact source checkout =="
checkout_exact "$HARLETTY_UPSTREAM" "$HARLETTY_COMMIT" "$HAR_DIR"
checkout_exact "$OMNIP_UPSTREAM" "$OMNIP_COMMIT" "$OMNI_DIR"

echo "== Aurora Pi5 runtime: Omniphony ARM64 portability fix =="
git -C "$OMNI_DIR" apply --check "$OMNIP_ARM64_PATCH"
git -C "$OMNI_DIR" apply "$OMNIP_ARM64_PATCH"

echo "== Aurora Pi5 runtime: Rust toolchain =="
rustup toolchain install "$TOOLCHAIN" --profile minimal >/dev/null

echo "== Aurora Pi5 runtime: Harletty $HARLETTY_VERSION =="
CARGO_TARGET_DIR="$BUILD_DIR/harletty"   cargo +"$TOOLCHAIN" build --release --locked   --manifest-path "$HAR_DIR/Cargo.toml" -p harletty-bridge

echo "== Aurora Pi5 runtime: Omniphony $OMNIP_VERSION with PipeWire =="
CARGO_TARGET_DIR="$BUILD_DIR/omniphony"   cargo +"$TOOLCHAIN" build --release --locked   --manifest-path "$OMNI_DIR/omniphony-renderer/Cargo.toml"   -p omniphony-renderer --features pipewire

install -m 0755 "$BUILD_DIR/omniphony/release/orender" "$PREFIX/bin/orender"
install -m 0644 "$BUILD_DIR/harletty/release/libharletty_bridge.so" "$PREFIX/lib/libharletty_bridge.so"
install -m 0644 "$ROOT_DIR/config/layouts/omniphony-11.1.4-aurora.yaml"   "$PREFIX/share/aurora/omniphony-11.1.4-aurora.yaml"

cat > "$PREFIX/share/aurora/runtime-versions.txt" <<EOF
harletty=$HARLETTY_VERSION
harletty_commit=$HARLETTY_COMMIT
omniphony=$OMNIP_VERSION
omniphony_commit=$OMNIP_COMMIT
EOF

"$PREFIX/bin/orender" --help >/dev/null
test -s "$PREFIX/lib/libharletty_bridge.so"

echo "AURORA-PI5-RUNTIME-BUILD-PASS prefix=$PREFIX"
