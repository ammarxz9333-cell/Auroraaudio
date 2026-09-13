#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONFIG="$ROOT_DIR/config/oar-evaluation-v1.json"
REFERENCE_ANALYZER="$ROOT_DIR/validation/open-immersive/oar_reference_evidence.py"
DIFFERENTIAL_ANALYZER="$ROOT_DIR/validation/open-immersive/oar_differential.py"
FIVE_ONE_ANALYZER="$ROOT_DIR/validation/open-immersive/oar_5_1_differential.py"
OAR_PROBE_SOURCE="$ROOT_DIR/validation/open-immersive/aurora_oar_differential_probe.c"
OAR_FIVE_ONE_PROBE_SOURCE="$ROOT_DIR/validation/open-immersive/aurora_oar_5_1_differential_probe.c"
OUTPUT_DIR=${1:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-oar-reference.XXXXXX")"}
SRC_DIR="$OUTPUT_DIR/oar"
BUILD_DIR="$OUTPUT_DIR/build"
CTEST_LOG="$OUTPUT_DIR/ctest.log"
REFERENCE_REPORT="$OUTPUT_DIR/oar-reference-evidence.json"
OAR_DIFFERENTIAL_JSON="$OUTPUT_DIR/oar-differential-oar.json"
AURORA_DIFFERENTIAL_JSON="$OUTPUT_DIR/oar-differential-aurora.json"
DIFFERENTIAL_REPORT="$OUTPUT_DIR/oar-differential-evidence.json"
OAR_FIVE_ONE_JSON="$OUTPUT_DIR/oar-5-1-differential-oar.json"
AURORA_FIVE_ONE_JSON="$OUTPUT_DIR/oar-5-1-differential-aurora.json"
FIVE_ONE_REPORT="$OUTPUT_DIR/oar-5-1-differential-evidence.json"

for cmd in git python3 cmake ctest cargo; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
for path in "$CONFIG" "$REFERENCE_ANALYZER" "$DIFFERENTIAL_ANALYZER" "$FIVE_ONE_ANALYZER" "$OAR_PROBE_SOURCE" "$OAR_FIVE_ONE_PROBE_SOURCE"; do
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

# Inject validation executables only into the temporary checkout. They are not
# registered with CTest, so the upstream 6/6 reference test set remains intact.
cp "$OAR_PROBE_SOURCE" "$SRC_DIR/tests/examples/aurora_oar_differential_probe.c"
cp "$OAR_FIVE_ONE_PROBE_SOURCE" "$SRC_DIR/tests/examples/aurora_oar_5_1_differential_probe.c"
cat >> "$SRC_DIR/tests/examples/CMakeLists.txt" <<'CMAKE'

# Aurora external-reference differential probes (temporary CI injection only).
add_executable(aurora_oar_differential_probe aurora_oar_differential_probe.c)
target_link_libraries(aurora_oar_differential_probe PRIVATE oar_test_common)
set_property(TARGET aurora_oar_differential_probe PROPERTY C_STANDARD 99)
add_executable(aurora_oar_5_1_differential_probe aurora_oar_5_1_differential_probe.c)
target_link_libraries(aurora_oar_5_1_differential_probe PRIVATE oar_test_common)
set_property(TARGET aurora_oar_5_1_differential_probe PROPERTY C_STANDARD 99)
CMAKE

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

echo "== Run pinned OAR stereo semantic probe =="
OAR_PROBE="$(find "$BUILD_DIR" -type f -name 'aurora_oar_differential_probe' -perm -u+x -print -quit)"
[[ -n "$OAR_PROBE" ]] || { echo "OAR differential probe executable not found" >&2; exit 1; }
"$OAR_PROBE" "$OAR_DIFFERENTIAL_JSON"

echo "== Run Aurora stereo semantic probe =="
(
  cd "$ROOT_DIR"
  cargo run --quiet --locked -p aurora-simulation-assurance \
    --bin aurora-oar-differential-probe -- \
    --output "$AURORA_DIFFERENTIAL_JSON"
)

echo "== Compare Aurora against pinned OAR stereo semantics =="
python3 "$DIFFERENTIAL_ANALYZER" analyze \
  --config "$CONFIG" \
  --aurora "$AURORA_DIFFERENTIAL_JSON" \
  --oar "$OAR_DIFFERENTIAL_JSON" \
  --output "$DIFFERENTIAL_REPORT"

echo "== Run pinned OAR 5.1 semantic probe =="
OAR_FIVE_ONE_PROBE="$(find "$BUILD_DIR" -type f -name 'aurora_oar_5_1_differential_probe' -perm -u+x -print -quit)"
[[ -n "$OAR_FIVE_ONE_PROBE" ]] || { echo "OAR 5.1 differential probe executable not found" >&2; exit 1; }
"$OAR_FIVE_ONE_PROBE" "$OAR_FIVE_ONE_JSON"

echo "== Run Aurora 5.1 semantic probe =="
(
  cd "$ROOT_DIR"
  cargo run --quiet --locked -p aurora-simulation-assurance \
    --bin aurora-oar-5-1-differential-probe -- \
    --output "$AURORA_FIVE_ONE_JSON"
)

echo "== Compare Aurora against pinned OAR 5.1 semantics =="
python3 "$FIVE_ONE_ANALYZER" analyze \
  --config "$CONFIG" \
  --aurora "$AURORA_FIVE_ONE_JSON" \
  --oar "$OAR_FIVE_ONE_JSON" \
  --output "$FIVE_ONE_REPORT"

echo "== Produce pinned-reference evidence =="
python3 "$REFERENCE_ANALYZER" analyze \
  --config "$CONFIG" \
  --repo "$SRC_DIR" \
  --ctest-log "$CTEST_LOG" \
  --output "$REFERENCE_REPORT"

echo "AURORA OAR REFERENCE PASS"
echo "reference_report=$REFERENCE_REPORT"
echo "stereo_differential_report=$DIFFERENTIAL_REPORT"
echo "five_one_differential_report=$FIVE_ONE_REPORT"
