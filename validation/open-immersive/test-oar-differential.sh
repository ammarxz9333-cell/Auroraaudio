#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT_DIR="${1:-${ROOT_DIR}/output/oar-reference}"
SRC_DIR="${OUTPUT_DIR}/oar"
BUILD_DIR="${OUTPUT_DIR}/build"
PROBE_SOURCE="${ROOT_DIR}/validation/open-immersive/oar_differential_probe.c"
PROBE_BIN="${OUTPUT_DIR}/oar-differential-probe"
OAR_TSV="${OUTPUT_DIR}/oar-differential.tsv"
AURORA_TSV="${OUTPUT_DIR}/aurora-differential.tsv"
REPORT="${OUTPUT_DIR}/oar-differential-evidence.json"
CONFIG="${ROOT_DIR}/config/oar-evaluation-v1.json"

if [[ ! -d "${SRC_DIR}" || ! -d "${BUILD_DIR}" ]]; then
  echo "Pinned OAR source/build missing. Run test-oar-reference.sh first." >&2
  exit 2
fi
if [[ ! -f "${BUILD_DIR}/liboar.so" ]]; then
  echo "Pinned OAR shared library missing: ${BUILD_DIR}/liboar.so" >&2
  exit 3
fi

mkdir -p "${OUTPUT_DIR}"

"${CC:-cc}" -std=c11 -O2 \
  -I"${SRC_DIR}/include" \
  -I"${SRC_DIR}/tests/examples" \
  "${PROBE_SOURCE}" \
  "${SRC_DIR}/tests/examples/test_helpers.c" \
  -L"${BUILD_DIR}" \
  -Wl,-rpath,"${BUILD_DIR}" \
  -Wl,-rpath,"${BUILD_DIR}/src/renderer/obr/obr_capi" \
  -loar -lm \
  -o "${PROBE_BIN}"

LD_LIBRARY_PATH="${BUILD_DIR}:${BUILD_DIR}/src/renderer/obr/obr_capi:${LD_LIBRARY_PATH:-}" \
  "${PROBE_BIN}" "${OAR_TSV}"

cargo run --quiet -p aurora-renderer-vbap --example oar_differential_probe -- \
  "${AURORA_TSV}"

python3 "${ROOT_DIR}/validation/open-immersive/oar_differential_evidence.py" analyze \
  --config "${CONFIG}" \
  --oar "${OAR_TSV}" \
  --aurora "${AURORA_TSV}" \
  --output "${REPORT}"

echo "OAR-AURORA-DIFFERENTIAL-PASS report=${REPORT}"
