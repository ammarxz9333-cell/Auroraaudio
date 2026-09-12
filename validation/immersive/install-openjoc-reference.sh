#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="$ROOT_DIR/config/external-components-v1.json"
DEST_DIR=${1:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-openjoc.XXXXXX")"}

fail() {
  echo "OPENJOC-INSTALL-FAIL: $*" >&2
  exit 1
}

[[ "$(uname -s)" == "Linux" ]] || fail "reference installer currently supports Linux only"
case "$(uname -m)" in
  x86_64|amd64) PLATFORM="x86_64-unknown-linux-gnu" ;;
  *) fail "unsupported architecture: $(uname -m)" ;;
esac

for cmd in curl python3 sha256sum tar; do
  command -v "$cmd" >/dev/null 2>&1 || fail "missing required command: $cmd"
done
[[ -f "$MANIFEST" ]] || fail "missing component manifest: $MANIFEST"
mkdir -p "$DEST_DIR"

readarray -t COMPONENT < <(python3 - "$MANIFEST" "$PLATFORM" <<'PY'
import json
import sys

manifest_path, platform = sys.argv[1:]
manifest = json.load(open(manifest_path, encoding="utf-8"))
components = {item["id"]: item for item in manifest["components"]}
try:
    component = components["openjoc"]
    artifact = component["release_artifacts"][platform]
except KeyError as exc:
    raise SystemExit(f"OPENJOC-INSTALL-FAIL: manifest is missing {exc}")
print(component["upstream"])
print(component["tested_version"])
print(artifact["name"])
print(artifact["sha256"])
PY
)

[[ ${#COMPONENT[@]} -eq 4 ]] || fail "could not resolve pinned OpenJOC artifact from manifest"
UPSTREAM=${COMPONENT[0]}
VERSION=${COMPONENT[1]}
ARTIFACT_NAME=${COMPONENT[2]}
ARTIFACT_SHA256=${COMPONENT[3]}

[[ "$UPSTREAM" == https://github.com/* ]] || fail "unsupported upstream URL: $UPSTREAM"
[[ "$ARTIFACT_SHA256" =~ ^[0-9a-f]{64}$ ]] || fail "invalid SHA-256 in manifest"

ARCHIVE="$DEST_DIR/$ARTIFACT_NAME"
EXTRACT_DIR="$DEST_DIR/extracted"
URL="$UPSTREAM/releases/download/v$VERSION/$ARTIFACT_NAME"
rm -rf "$EXTRACT_DIR"
mkdir -p "$EXTRACT_DIR"

echo "Downloading pinned OpenJOC $VERSION for $PLATFORM" >&2
curl --fail --location --silent --show-error "$URL" --output "$ARCHIVE"
printf '%s  %s\n' "$ARTIFACT_SHA256" "$ARCHIVE" | sha256sum --check --status || fail "release artifact checksum mismatch"
echo "OpenJOC artifact checksum PASS: $ARTIFACT_SHA256" >&2

tar -xzf "$ARCHIVE" -C "$EXTRACT_DIR"
mapfile -t CANDIDATES < <(find "$EXTRACT_DIR" -type f -name openjoc -print)
[[ ${#CANDIDATES[@]} -eq 1 ]] || fail "expected exactly one openjoc executable in archive, found ${#CANDIDATES[@]}"
OPENJOC_BIN=${CANDIDATES[0]}
chmod +x "$OPENJOC_BIN"

VERSION_TEXT=$($OPENJOC_BIN --version 2>&1) || fail "downloaded openjoc executable did not run"
[[ "$VERSION_TEXT" == *"$VERSION"* ]] || fail "downloaded binary version mismatch: expected $VERSION, got $VERSION_TEXT"
echo "OpenJOC binary verification PASS: $VERSION_TEXT" >&2

printf '%s\n' "$OPENJOC_BIN"
