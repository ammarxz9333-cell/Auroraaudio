#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REFERENCE_RUNNER="$ROOT_DIR/validation/open-immersive/test-oar-reference.sh"
OAR_PROBE_SOURCE="$ROOT_DIR/validation/open-immersive/oar_7_1_4_object_probe.c"
ANALYZER="$ROOT_DIR/validation/open-immersive/oar_aurora_714_differential.py"
OUTPUT_DIR=${1:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-oar-714-differential.XXXXXX")"}
REFERENCE_DIR="$OUTPUT_DIR/reference"
OAR_PROBE="$OUTPUT_DIR/oar-714-object-probe"
OAR_LOG="$OUTPUT_DIR/oar-714-probe.log"
AURORA_LOG="$OUTPUT_DIR/aurora-714-probe.log"
REPORT="$OUTPUT_DIR/oar-aurora-714-differential.json"

for cmd in bash python3 cc cargo; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
for path in "$REFERENCE_RUNNER" "$OAR_PROBE_SOURCE" "$ANALYZER"; do
  [[ -f "$path" ]] || { echo "missing required file: $path" >&2; exit 2; }
done

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

echo "== Reproduce exact pinned OAR reference =="
bash "$REFERENCE_RUNNER" "$REFERENCE_DIR"
OAR_SRC="$REFERENCE_DIR/oar"
OAR_BUILD="$REFERENCE_DIR/build"
[[ -f "$OAR_BUILD/liboar.so" ]] || { echo "pinned OAR shared library missing: $OAR_BUILD/liboar.so" >&2; exit 1; }

echo "== Build OAR 7.1.4 object semantics probe =="
cc -std=c11 -O2 "$OAR_PROBE_SOURCE" \
  -I"$OAR_SRC/include" \
  -L"$OAR_BUILD" \
  -Wl,-rpath,"$OAR_BUILD" \
  -loar -lm \
  -o "$OAR_PROBE"

echo "== Run OAR 7.1.4 object semantics probe =="
"$OAR_PROBE" | tee "$OAR_LOG"

echo "== Run Aurora 3D VBAP 7.1.4 semantics probe =="
CARGO_TARGET_DIR="$OUTPUT_DIR/aurora-target" \
  cargo run --quiet --release -p aurora-renderer-vbap --example oar_7_1_4_probe \
  | tee "$AURORA_LOG"

echo "== Compare 7.1.4 object semantics =="
python3 "$ANALYZER" self-test
python3 "$ANALYZER" analyze \
  --aurora-log "$AURORA_LOG" \
  --oar-log "$OAR_LOG" \
  --output "$REPORT"

echo "AURORA OAR 7.1.4 DIFFERENTIAL PASS"
echo "report=$REPORT"
