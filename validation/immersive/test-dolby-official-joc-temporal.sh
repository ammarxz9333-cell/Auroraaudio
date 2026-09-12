#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT_DIR=${1:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-dolby-joc.XXXXXX")"}
TEMPORAL_HARNESS="$ROOT_DIR/validation/immersive/test-joc-temporal-evidence.sh"
OPENJOC_BIN=${OPENJOC_BIN:-openjoc}

URL="https://ott.dolby.com/OnDelKits/DDP/Dolby_Digital_Plus_Online_Delivery_Kit_v1.4.1/Test_Signals/elementary_streams/audio.zip"
NAME="Living-Room-Atmos_6ch_640kbps_ddp_joc.ec3"
EXPECTED_ZIP_SHA256="f94d5e3e933f756856686546763f42a8a5f16b10c264fc7af1d228acc09baa62"
EXPECTED_SOURCE_SHA256="2470373db2c3621d56a2852df070e140293e9a99fdaa07e5c06de3c86bec307f"
EXPECTED_DERIVED_SHA256="0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0"
EXPECTED_FIRST_AU_BYTES=2560

fail() {
  echo "DOLBY-JOC-TEMPORAL-FAIL: $*" >&2
  exit 2
}

for cmd in curl sha256sum python3; do
  command -v "$cmd" >/dev/null 2>&1 || fail "missing required command: $cmd"
done
command -v "$OPENJOC_BIN" >/dev/null 2>&1 || fail "OpenJOC executable not found: $OPENJOC_BIN"
[[ -f "$TEMPORAL_HARNESS" ]] || fail "temporal harness missing: $TEMPORAL_HARNESS"

mkdir -p "$OUTPUT_DIR"
ZIP="$OUTPUT_DIR/audio.zip"
SOURCE="$OUTPUT_DIR/$NAME"
DERIVED="$OUTPUT_DIR/Living-Room-Atmos_after_first_au.ec3"
DERIVATION="$OUTPUT_DIR/derivation.json"
ORIGINAL_INSPECT="$OUTPUT_DIR/original-openjoc-inspect.json"
TEMPORAL_DIR="$OUTPUT_DIR/temporal"
REPORT="$TEMPORAL_DIR/joc-temporal-evidence.json"

printf '\n== Aurora Dolby JOC phase: acquire checksum-pinned official test vector ==\n'
curl --fail --location --retry 3 --silent --show-error "$URL" --output "$ZIP"
ZIP_SHA256="$(sha256sum "$ZIP" | awk '{print $1}')"
[[ "$ZIP_SHA256" == "$EXPECTED_ZIP_SHA256" ]] || \
  fail "Dolby archive SHA-256 mismatch: expected $EXPECTED_ZIP_SHA256 got $ZIP_SHA256"

python3 - "$ZIP" "$SOURCE" "$NAME" <<'PY'
import pathlib
import sys
import zipfile

archive = pathlib.Path(sys.argv[1])
output = pathlib.Path(sys.argv[2])
wanted = sys.argv[3]
with zipfile.ZipFile(archive) as zf:
    matches = [name for name in zf.namelist() if pathlib.PurePosixPath(name).name == wanted]
    if len(matches) != 1:
        raise SystemExit(f"expected exactly one {wanted!r}, found {matches!r}")
    output.write_bytes(zf.read(matches[0]))
PY

SOURCE_SHA256="$(sha256sum "$SOURCE" | awk '{print $1}')"
[[ "$SOURCE_SHA256" == "$EXPECTED_SOURCE_SHA256" ]] || \
  fail "Dolby carrier SHA-256 mismatch: expected $EXPECTED_SOURCE_SHA256 got $SOURCE_SHA256"

printf '\n== Aurora Dolby JOC phase: pin untouched-carrier OpenJOC 0.17.0 boundary ==\n'
"$OPENJOC_BIN" inspect "$SOURCE" --json --objects --emdf --aus >"$ORIGINAL_INSPECT" || \
  fail "OpenJOC inspect command failed on untouched carrier"

python3 - "$ORIGINAL_INSPECT" <<'PY'
import json
import pathlib
import sys

payload = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
eac3 = payload.get("eac3") or {}
joc = payload.get("joc") or {}
validation = payload.get("validation") or {}
diagnostics = payload.get("diagnostics") or {}
compat = validation.get("deployed_compatibility") or {}
first = diagnostics.get("first_failure") or {}

checks = [
    (joc.get("present") is True, "untouched carrier was not identified as JOC"),
    (int(eac3.get("access_unit_count", 0)) == 2361, "unexpected untouched AU count"),
    (validation.get("stream_parse") == "fail", "OpenJOC boundary unexpectedly changed: stream_parse no longer fail"),
    (int(validation.get("malformed_aus", 0)) == 1, "OpenJOC boundary unexpectedly changed: malformed AU count"),
    (compat.get("status") == "pass", "deployed compatibility gate did not pass on untouched carrier"),
    (diagnostics.get("complete") is True, "untouched-carrier diagnostics incomplete"),
    (int(diagnostics.get("issue_count", 0)) == 1, "unexpected untouched-carrier diagnostic count"),
    (first.get("code") == "MALFORMED_OAMD_METADATA", "unexpected first diagnostic code"),
    (int(first.get("au", -1)) == 0, "OpenJOC boundary is not isolated to AU0"),
    (first.get("message") == "reserved OAMD object size index 3", "unexpected AU0 diagnostic message"),
]
for passed, message in checks:
    if not passed:
        raise SystemExit(message)
print("DOLBY-JOC-UNTOUCHED-OPENJOC-BOUNDARY-PASS au_count=2361 issue_count=1 failing_au=0")
PY

printf '\n== Aurora Dolby JOC phase: derive byte-identical suffix after AU0 ==\n'
python3 - "$SOURCE" "$DERIVED" "$DERIVATION" "$EXPECTED_FIRST_AU_BYTES" <<'PY'
import hashlib
import json
import pathlib
import sys

source = pathlib.Path(sys.argv[1])
derived = pathlib.Path(sys.argv[2])
manifest = pathlib.Path(sys.argv[3])
expected_first_au_bytes = int(sys.argv[4])
data = source.read_bytes()
if len(data) < 8 or data[:2] != b"\x0b\x77":
    raise SystemExit("unexpected E-AC-3 syncword")

# ETSI TS 102 366 E-AC-3 frmsiz: 11-bit value in 16-bit words minus one.
frmsiz = ((data[2] & 0x07) << 8) | data[3]
first_au_bytes = 2 * (frmsiz + 1)
if first_au_bytes != expected_first_au_bytes:
    raise SystemExit(
        f"unexpected first AU size: expected {expected_first_au_bytes}, got {first_au_bytes}"
    )
remainder = data[first_au_bytes:]
if not remainder or remainder[:2] != b"\x0b\x77":
    raise SystemExit("AU0 removal did not land on the next E-AC-3 syncword")

derived.write_bytes(remainder)
payload = {
    "schema_version": 1,
    "source_sha256": hashlib.sha256(data).hexdigest(),
    "source_bytes": len(data),
    "transform": "drop_exactly_first_raw_eac3_access_unit_no_reencode",
    "dropped_access_units": 1,
    "dropped_bytes": first_au_bytes,
    "derived_sha256": hashlib.sha256(remainder).hexdigest(),
    "derived_bytes": len(remainder),
    "byte_identity_check": remainder == data[first_au_bytes:],
    "truth_boundary": (
        "Derived carrier is a byte-identical suffix of the official Dolby vector. "
        "It is not the untouched official carrier and is not Dolby conformance evidence."
    ),
}
manifest.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(
    "DOLBY-JOC-DERIVATION-PASS "
    f"dropped_bytes={first_au_bytes} derived_sha256={payload['derived_sha256']}"
)
PY

DERIVED_SHA256="$(sha256sum "$DERIVED" | awk '{print $1}')"
[[ "$DERIVED_SHA256" == "$EXPECTED_DERIVED_SHA256" ]] || \
  fail "derived carrier SHA-256 mismatch: expected $EXPECTED_DERIVED_SHA256 got $DERIVED_SHA256"

printf '\n== Aurora Dolby JOC phase: positive moving-object temporal proof on pinned suffix ==\n'
bash "$TEMPORAL_HARNESS" \
  "$DERIVED" \
  "$EXPECTED_DERIVED_SHA256" \
  "Official Dolby DDP Online Delivery Kit v1.4.1 Living-Room JOC vector; original SHA-256 $EXPECTED_SOURCE_SHA256; byte-identical suffix after dropping exactly AU0 (2560 bytes) because OpenJOC 0.17.0 classifies that AU's OAMD size index 3 as reserved; no re-encoding" \
  "$TEMPORAL_DIR"

[[ -s "$REPORT" ]] || fail "temporal evidence report missing"
python3 - "$REPORT" "$DERIVATION" <<'PY'
import json
import pathlib
import sys

report = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
derivation = json.loads(pathlib.Path(sys.argv[2]).read_text(encoding="utf-8"))
codec = report.get("codec_joc") or {}
objects = report.get("object_metadata") or {}
rendered = report.get("rendered_7_1_4") or {}

checks = [
    (derivation.get("byte_identity_check") is True, "derived suffix is not byte-identical"),
    (report.get("verdict") == "pass", f"temporal verdict is {report.get('verdict')!r}"),
    (not report.get("failures"), "temporal report contains failures"),
    (codec.get("gate_pass") is True, "codec/JOC gate did not pass"),
    (int(codec.get("access_unit_count", 0)) == 2360, "unexpected derived AU count"),
    (codec.get("stream_parse") == "pass", "derived stream_parse did not pass"),
    (codec.get("decoder_admissible") is True, "derived carrier not decoder-admissible"),
    (objects.get("dynamic_metadata_detected") is True, "dynamic metadata was not detected"),
    (len(objects.get("dynamic_object_indices") or []) == 15, "expected 15 dynamic object indices"),
    (int(objects.get("metadata_update_count", 0)) == 37760, "unexpected metadata update count"),
    (int(rendered.get("channels", 0)) == 12, "expected 12 rendered channels"),
    (rendered.get("temporal_diversity") is True, "rendered temporal diversity did not pass"),
    (float(rendered.get("duration_seconds", 0.0)) > 75.0, "rendered duration too short"),
]
for passed, message in checks:
    if not passed:
        raise SystemExit(message)

print(
    "DOLBY-JOC-MOVING-TEMPORAL-PASS "
    f"aus={codec['access_unit_count']} metadata_updates={objects['metadata_update_count']} "
    f"dynamic_objects={len(objects['dynamic_object_indices'])} channels={rendered['channels']} "
    f"duration_seconds={rendered['duration_seconds']:.6f}"
)
PY

echo "AURORA-DOLBY-JOC-TEMPORAL-PROOF-PASS"
echo "artifacts: $OUTPUT_DIR"
