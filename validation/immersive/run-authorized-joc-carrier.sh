#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
  echo "usage: $0 INPUT EXPECTED_SOURCE_SHA256 PROVENANCE [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INPUT=$1
EXPECTED_SOURCE_SHA256=$(printf '%s' "$2" | tr '[:upper:]' '[:lower:]')
PROVENANCE=$3
OUTPUT_DIR=${4:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-authorized-joc.XXXXXX")"}
FFMPEG_BIN=${FFMPEG_BIN:-ffmpeg}
FFPROBE_BIN=${FFPROBE_BIN:-ffprobe}
TEMPORAL_HARNESS="$ROOT_DIR/validation/immersive/test-joc-temporal-evidence.sh"

fail() {
  echo "AUTHORIZED-JOC-CARRIER-FAIL: $*" >&2
  exit 2
}

[[ -f "$INPUT" ]] || fail "input not found: $INPUT"
[[ -f "$TEMPORAL_HARNESS" ]] || fail "temporal harness missing: $TEMPORAL_HARNESS"
[[ "$EXPECTED_SOURCE_SHA256" =~ ^[0-9a-f]{64}$ ]] || \
  fail "EXPECTED_SOURCE_SHA256 must be 64 hex digits"
[[ -n "${PROVENANCE//[[:space:]]/}" ]] || fail "PROVENANCE must be non-empty"

for cmd in sha256sum python3 "$FFMPEG_BIN" "$FFPROBE_BIN"; do
  command -v "$cmd" >/dev/null 2>&1 || fail "missing required command: $cmd"
done

ACTUAL_SOURCE_SHA256=$(sha256sum "$INPUT" | awk '{print $1}')
[[ "$ACTUAL_SOURCE_SHA256" == "$EXPECTED_SOURCE_SHA256" ]] || \
  fail "source SHA-256 mismatch: expected $EXPECTED_SOURCE_SHA256 got $ACTUAL_SOURCE_SHA256"

mkdir -p "$OUTPUT_DIR"
PROBE_JSON="$OUTPUT_DIR/source-ffprobe.json"
EXTRACTED_JOC="$OUTPUT_DIR/carrier.eac3"
MANIFEST_JSON="$OUTPUT_DIR/authorized-carrier-manifest.json"
TEMPORAL_DIR="$OUTPUT_DIR/temporal"

"$FFPROBE_BIN" -v error \
  -show_entries format=format_name,duration:stream=index,codec_type,codec_name,channels,sample_rate \
  -of json "$INPUT" >"$PROBE_JSON" || fail "ffprobe failed for input"

read -r AUDIO_INDEX FORMAT_NAME < <(python3 - "$PROBE_JSON" <<'PY'
import json
import pathlib
import sys

payload = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
streams = payload.get("streams") or []
eac3 = [
    stream for stream in streams
    if stream.get("codec_type") == "audio" and stream.get("codec_name") == "eac3"
]
if len(eac3) != 1:
    raise SystemExit(
        f"AUTHORIZED-JOC-CARRIER-FAIL: expected exactly one E-AC-3 audio stream, found {len(eac3)}"
    )
index = eac3[0].get("index")
if index is None:
    raise SystemExit("AUTHORIZED-JOC-CARRIER-FAIL: E-AC-3 stream has no ffprobe index")
format_name = (payload.get("format") or {}).get("format_name") or "unknown"
print(int(index), format_name)
PY
) || fail "input stream contract validation failed"

if [[ ",${FORMAT_NAME}," == *",eac3,"* ]]; then
  cp -- "$INPUT" "$EXTRACTED_JOC"
else
  "$FFMPEG_BIN" -hide_banner -loglevel error -y -i "$INPUT" \
    -map "0:${AUDIO_INDEX}" -c copy -f eac3 "$EXTRACTED_JOC" || \
    fail "failed to extract E-AC-3 by stream copy"
fi

[[ -s "$EXTRACTED_JOC" ]] || fail "extracted E-AC-3 stream is empty"
EXTRACTED_SHA256=$(sha256sum "$EXTRACTED_JOC" | awk '{print $1}')

if [[ -n "${EXPECTED_JOC_SHA256:-}" ]]; then
  EXPECTED_JOC_SHA256_NORMALIZED=$(printf '%s' "$EXPECTED_JOC_SHA256" | tr '[:upper:]' '[:lower:]')
  [[ "$EXPECTED_JOC_SHA256_NORMALIZED" =~ ^[0-9a-f]{64}$ ]] || \
    fail "EXPECTED_JOC_SHA256 must be 64 hex digits when set"
  [[ "$EXTRACTED_SHA256" == "$EXPECTED_JOC_SHA256_NORMALIZED" ]] || \
    fail "extracted JOC SHA-256 mismatch: expected $EXPECTED_JOC_SHA256_NORMALIZED got $EXTRACTED_SHA256"
fi

python3 - \
  "$INPUT" \
  "$EXPECTED_SOURCE_SHA256" \
  "$EXTRACTED_JOC" \
  "$EXTRACTED_SHA256" \
  "$PROVENANCE" \
  "$PROBE_JSON" \
  "$MANIFEST_JSON" <<'PY'
import json
import pathlib
import sys

(
    input_path,
    source_sha256,
    extracted_path,
    extracted_sha256,
    provenance,
    probe_path,
    output_path,
) = sys.argv[1:]
probe = json.loads(pathlib.Path(probe_path).read_text(encoding="utf-8"))
payload = {
    "schema_version": 1,
    "source": {
        "path": str(pathlib.Path(input_path).resolve()),
        "sha256": source_sha256,
        "provenance": provenance,
    },
    "extracted_joc": {
        "path": str(pathlib.Path(extracted_path).resolve()),
        "sha256": extracted_sha256,
        "method": "direct-copy" if "eac3" in ((probe.get("format") or {}).get("format_name") or "").split(",") else "ffmpeg-stream-copy",
        "reencoded": False,
    },
    "ffprobe": probe,
}
pathlib.Path(output_path).write_text(
    json.dumps(payload, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY

printf '\n== Aurora authorized JOC carrier ==\n'
printf 'source_sha256=%s\n' "$EXPECTED_SOURCE_SHA256"
printf 'extracted_joc_sha256=%s\n' "$EXTRACTED_SHA256"
printf 'manifest=%s\n' "$MANIFEST_JSON"

set +e
bash "$TEMPORAL_HARNESS" \
  "$EXTRACTED_JOC" \
  "$EXTRACTED_SHA256" \
  "$PROVENANCE; source_sha256=$EXPECTED_SOURCE_SHA256; extraction=stream-copy" \
  "$TEMPORAL_DIR"
HARNESS_RC=$?
set -e

case "$HARNESS_RC" in
  0)
    echo "AUTHORIZED-JOC-CARRIER-PASS"
    echo "evidence: $OUTPUT_DIR"
    ;;
  3)
    echo "AUTHORIZED-JOC-CARRIER-INSUFFICIENT: codec/JOC passed but moving-object temporal diversity was not established" >&2
    echo "evidence: $OUTPUT_DIR" >&2
    ;;
  *)
    echo "AUTHORIZED-JOC-CARRIER-FAIL: temporal harness rejected the carrier (rc=$HARNESS_RC)" >&2
    echo "evidence: $OUTPUT_DIR" >&2
    ;;
esac

exit "$HARNESS_RC"
