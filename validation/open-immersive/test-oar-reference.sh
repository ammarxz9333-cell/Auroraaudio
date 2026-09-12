#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONFIG="$ROOT_DIR/config/oar-evaluation-v1.json"
ANALYZER="$ROOT_DIR/validation/open-immersive/oar_reference_evidence.py"
OUTPUT_DIR=${1:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-oar-reference.XXXXXX")"}
SRC_DIR="$OUTPUT_DIR/oar"
BUILD_DIR="$OUTPUT_DIR/build"
CTEST_LOG="$OUTPUT_DIR/ctest.log"
REPORT="$OUTPUT_DIR/oar-reference-evidence.json"

for cmd in git python3 cmake ctest; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
for path in "$CONFIG" "$ANALYZER"; do
  [[ -f "$path" ]] || { echo "missing required file: $path" >&2; exit 2; }
done

readarray -t REF < <(python3 - "$CONFIG" <<'PY'
import json, sys
p = json.load(open(sys.argv[1], encoding="utf-8"))["reference"]
print(p["upstream"])
print(p["commit"])
print("ON" if p["build"]["build_examples"] else "OFF")
print("ON" if p["build"]["enable_binauralizer"] else "OFF")
print("ON" if p["build"]["enable_hoa_lfe"] else "OFF")
PY
)
UPSTREAM=${REF[0]}
COMMIT=${REF[1]}
BUILD_EXAMPLES=${REF[2]}
ENABLE_BINAURALIZER=${REF[3]}
ENABLE_HOA_LFE=${REF[4]}

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

echo "== Clone exact OAR reference =="
git clone --quiet --filter=blob:none --no-checkout "$UPSTREAM" "$SRC_DIR"
git -C "$SRC_DIR" fetch --quiet --depth 1 origin "$COMMIT"
git -C "$SRC_DIR" checkout --quiet --detach "$COMMIT"
ACTUAL_COMMIT="$(git -C "$SRC_DIR" rev-parse HEAD)"
[[ "$ACTUAL_COMMIT" == "$COMMIT" ]] || {
  echo "OAR commit mismatch: expected $COMMIT got $ACTUAL_COMMIT" >&2
  exit 1
}
[[ -f "$SRC_DIR/LICENSE" ]] || { echo "OAR LICENSE missing" >&2; exit 1; }
[[ -f "$SRC_DIR/PATENTS" ]] || { echo "OAR PATENTS missing" >&2; exit 1; }

echo "== Configure pinned OAR =="
cmake -S "$SRC_DIR" -B "$BUILD_DIR" \
  -DCMAKE_BUILD_TYPE=Release \
  -DOAR_BUILD_EXAMPLES="$BUILD_EXAMPLES" \
  -DOAR_ENABLE_BINAURALIZER="$ENABLE_BINAURALIZER" \
  -DOAR_ENABLE_HOA_LFE="$ENABLE_HOA_LFE"

echo "== Build pinned OAR =="
cmake --build "$BUILD_DIR" --parallel 2

echo "== Run upstream OAR tests =="
ctest --test-dir "$BUILD_DIR" --output-on-failure --output-log "$CTEST_LOG"

echo "== Produce Aurora evidence =="
python3 "$ANALYZER" analyze \
  --config "$CONFIG" \
  --repo "$SRC_DIR" \
  --ctest-log "$CTEST_LOG" \
  --output "$REPORT"

echo "AURORA OAR REFERENCE PASS"
echo "report=$REPORT"
