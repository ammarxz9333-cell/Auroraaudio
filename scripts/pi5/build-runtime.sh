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

for cmd in git python3 cargo rustup curl tar sha256sum; do
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

cam=c["camilladsp"]
print(f"CAMILLA_UPSTREAM={shlex.quote(cam['upstream'])}")
print(f"CAMILLA_VERSION={shlex.quote(cam['tested_version'])}")
arch={"aarch64":"aarch64-unknown-linux-gnu","x86_64":"x86_64-unknown-linux-gnu"}.get(__import__("platform").machine())
if not arch:
    raise SystemExit(f"unsupported CamillaDSP build host architecture: {__import__('platform').machine()}")
artifact=cam["release_artifacts"][arch]
print(f"CAMILLA_ASSET={shlex.quote(artifact['name'])}")
print(f"CAMILLA_SHA256={shlex.quote(artifact['sha256'])}")
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

echo "== Aurora Pi5 runtime: CamillaDSP $CAMILLA_VERSION =="
CAMILLA_CACHE="$BUILD_DIR/$CAMILLA_ASSET"
CAMILLA_URL="$CAMILLA_UPSTREAM/releases/download/v$CAMILLA_VERSION/$CAMILLA_ASSET"
if [[ ! -f "$CAMILLA_CACHE" ]] || ! printf '%s  %s\n' "$CAMILLA_SHA256" "$CAMILLA_CACHE" | sha256sum -c - >/dev/null 2>&1; then
  rm -f "$CAMILLA_CACHE"
  curl -fL --retry 3 --retry-delay 1 "$CAMILLA_URL" -o "$CAMILLA_CACHE"
fi
printf '%s  %s\n' "$CAMILLA_SHA256" "$CAMILLA_CACHE" | sha256sum -c -
CAMILLA_EXTRACT="$BUILD_DIR/camilladsp-$CAMILLA_VERSION"
rm -rf "$CAMILLA_EXTRACT"
mkdir -p "$CAMILLA_EXTRACT"
tar -xzf "$CAMILLA_CACHE" -C "$CAMILLA_EXTRACT"
CAMILLA_BIN="$(find "$CAMILLA_EXTRACT" -type f -name camilladsp -perm -u+x | head -n1)"
[[ -n "$CAMILLA_BIN" ]] || { echo "CamillaDSP binary missing from $CAMILLA_ASSET" >&2; exit 1; }
install -m 0755 "$CAMILLA_BIN" "$PREFIX/bin/camilladsp"

cat > "$PREFIX/share/aurora/runtime-versions.txt" <<EOF
harletty=$HARLETTY_VERSION
harletty_commit=$HARLETTY_COMMIT
omniphony=$OMNIP_VERSION
omniphony_commit=$OMNIP_COMMIT
camilladsp=$CAMILLA_VERSION
camilladsp_asset=$CAMILLA_ASSET
camilladsp_sha256=$CAMILLA_SHA256
EOF

"$PREFIX/bin/orender" --help >/dev/null
"$PREFIX/bin/camilladsp" --version
test -s "$PREFIX/lib/libharletty_bridge.so"

echo "AURORA-PI5-RUNTIME-BUILD-PASS prefix=$PREFIX"
