#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONFIG="$ROOT_DIR/config/oar-evaluation-v1.json"
OUTPUT_DIR=${1:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-oar-reference.XXXXXX")"}
SOURCE_DIR="$OUTPUT_DIR/oar"
BUILD_DIR="$OUTPUT_DIR/build"
CTEST_XML="$OUTPUT_DIR/ctest.xml"
REPORT="$OUTPUT_DIR/oar-reference-evidence.json"

for cmd in git cmake python3; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
[[ -f "$CONFIG" ]] || { echo "missing OAR evaluation config: $CONFIG" >&2; exit 2; }
mkdir -p "$OUTPUT_DIR"

readarray -t FIELDS < <(python3 - "$CONFIG" <<'PY'
import json, sys
p = json.load(open(sys.argv[1], encoding="utf-8"))["reference"]
for key in ("upstream", "version", "commit", "license"):
    print(p[key])
print("ON" if p["build"]["build_examples"] else "OFF")
print("ON" if p["build"]["enable_binauralizer"] else "OFF")
print("ON" if p["build"]["enable_hoa_lfe"] else "OFF")
PY
)
UPSTREAM=${FIELDS[0]}
VERSION=${FIELDS[1]}
COMMIT=${FIELDS[2]}
EXPECTED_LICENSE=${FIELDS[3]}
BUILD_EXAMPLES=${FIELDS[4]}
ENABLE_BINAURALIZER=${FIELDS[5]}
ENABLE_HOA_LFE=${FIELDS[6]}

printf '\n== OAR reference: clone exact pinned commit ==\n'
rm -rf "$SOURCE_DIR" "$BUILD_DIR"
git clone --quiet --filter=blob:none --no-checkout "$UPSTREAM" "$SOURCE_DIR"
git -C "$SOURCE_DIR" fetch --quiet --depth 1 origin "$COMMIT"
git -C "$SOURCE_DIR" checkout --quiet --detach "$COMMIT"
ACTUAL_COMMIT="$(git -C "$SOURCE_DIR" rev-parse HEAD)"
[[ "$ACTUAL_COMMIT" == "$COMMIT" ]] || {
  echo "OAR commit mismatch: expected $COMMIT got $ACTUAL_COMMIT" >&2
  exit 1
}
[[ -s "$SOURCE_DIR/LICENSE" ]] || { echo "OAR LICENSE missing" >&2; exit 1; }
[[ -s "$SOURCE_DIR/PATENTS" ]] || { echo "OAR PATENTS missing" >&2; exit 1; }
grep -q "BSD 3-Clause Clear License" "$SOURCE_DIR/LICENSE" || {
  echo "OAR license no longer matches $EXPECTED_LICENSE expectation" >&2
  exit 1
}
grep -q "Alliance for Open Media Patent License" "$SOURCE_DIR/PATENTS" || {
  echo "OAR patent license file shape changed" >&2
  exit 1
}

printf '\n== OAR reference: configure pinned source ==\n'
cmake -S "$SOURCE_DIR" -B "$BUILD_DIR" \
  -DCMAKE_BUILD_TYPE=Release \
  -DOAR_BUILD_SHARED_LIBRARY=OFF \
  -DOAR_BUILD_EXAMPLES="$BUILD_EXAMPLES" \
  -DOAR_ENABLE_BINAURALIZER="$ENABLE_BINAURALIZER" \
  -DOAR_ENABLE_HOA_LFE="$ENABLE_HOA_LFE"

printf '\n== OAR reference: build ==\n'
cmake --build "$BUILD_DIR" --parallel 2

printf '\n== OAR reference: upstream tests ==\n'
ctest --test-dir "$BUILD_DIR" --output-on-failure --output-junit "$CTEST_XML"
[[ -s "$CTEST_XML" ]] || { echo "OAR ctest XML was not produced" >&2; exit 1; }

printf '\n== OAR reference: write evidence ==\n'
python3 - "$CONFIG" "$SOURCE_DIR" "$CTEST_XML" "$REPORT" <<'PY'
import hashlib
import json
import pathlib
import subprocess
import sys
import xml.etree.ElementTree as ET

config_path = pathlib.Path(sys.argv[1])
source = pathlib.Path(sys.argv[2])
xml_path = pathlib.Path(sys.argv[3])
report_path = pathlib.Path(sys.argv[4])
config = json.loads(config_path.read_text(encoding="utf-8"))
ref = config["reference"]
actual = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()

def sha256(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

root = ET.parse(xml_path).getroot()
# CTest may emit either <testsuite> or <testsuites><testsuite>.
suites = [root] if root.tag == "testsuite" else list(root.findall("testsuite"))
if not suites:
    raise SystemExit("no test suites in CTest XML")
tests = sum(int(s.attrib.get("tests", "0")) for s in suites)
failures = sum(int(s.attrib.get("failures", "0")) for s in suites)
errors = sum(int(s.attrib.get("errors", "0")) for s in suites)
skipped = sum(int(s.attrib.get("skipped", "0")) for s in suites)
if tests <= 0:
    raise SystemExit("pinned OAR upstream test suite reported zero tests")
if failures or errors:
    raise SystemExit(f"OAR upstream tests not clean: failures={failures} errors={errors}")

payload = {
    "schema_version": 1,
    "verdict": "pass",
    "reference": {
        "id": ref["id"],
        "upstream": ref["upstream"],
        "version": ref["version"],
        "expected_commit": ref["commit"],
        "actual_commit": actual,
        "license": ref["license"],
        "license_sha256": sha256(source / "LICENSE"),
        "patents_sha256": sha256(source / "PATENTS"),
        "integration": ref["integration"],
    },
    "upstream_tests": {
        "tests": tests,
        "failures": failures,
        "errors": errors,
        "skipped": skipped,
    },
    "coverage_promotion": "not_yet_eligible_without_aurora_differential_evidence",
    "truth_boundary": config["truth_boundary"],
}
report_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(
    "AURORA-OAR-REFERENCE-PASS "
    f"commit={actual} tests={tests} skipped={skipped} failures={failures} errors={errors}"
)
print(f"report={report_path}")
PY
