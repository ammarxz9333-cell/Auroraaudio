#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
  echo "usage: $0 INPUT_JOC EXPECTED_SHA256 PROVENANCE [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INPUT=$1
EXPECTED_SHA256=$(printf '%s' "$2" | tr '[:upper:]' '[:lower:]')
PROVENANCE=$3
OUTPUT_DIR=${4:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-joc-temporal.XXXXXX")"}
REFERENCE="$ROOT_DIR/validation/immersive/test-openjoc-reference.sh"
ANALYZER="$ROOT_DIR/validation/immersive/joc_temporal_evidence.py"
FFMPEG_BIN=${FFMPEG_BIN:-ffmpeg}

fail() {
  echo "JOC-TEMPORAL-HARNESS-FAIL: $*" >&2
  exit 2
}

[[ -f "$INPUT" ]] || fail "input not found: $INPUT"
[[ -f "$REFERENCE" ]] || fail "OpenJOC reference harness missing: $REFERENCE"
[[ -f "$ANALYZER" ]] || fail "temporal analyzer missing: $ANALYZER"
[[ "$EXPECTED_SHA256" =~ ^[0-9a-f]{64}$ ]] || fail "EXPECTED_SHA256 must be 64 hex digits"
[[ -n "${PROVENANCE//[[:space:]]/}" ]] || fail "PROVENANCE must be non-empty"
for cmd in sha256sum python3 "$FFMPEG_BIN"; do
  command -v "$cmd" >/dev/null 2>&1 || fail "missing required command: $cmd"
done

ACTUAL_SHA256=$(sha256sum "$INPUT" | awk '{print $1}')
[[ "$ACTUAL_SHA256" == "$EXPECTED_SHA256" ]] || \
  fail "input SHA-256 mismatch: expected $EXPECTED_SHA256 got $ACTUAL_SHA256"

mkdir -p "$OUTPUT_DIR"
REFERENCE_DIR="$OUTPUT_DIR/openjoc"
REPORT="$OUTPUT_DIR/joc-temporal-evidence.json"
PCM="$OUTPUT_DIR/openjoc-7.1.4.f32"

printf '\n== Aurora temporal JOC phase: OpenJOC codec/object census ==\n'
bash "$REFERENCE" "$INPUT" "$REFERENCE_DIR"

PROBE_JSON="$REFERENCE_DIR/openjoc-7.1.4-ffprobe.json"
INSPECT_JSON="$REFERENCE_DIR/openjoc-inspect.json"
WAV="$REFERENCE_DIR/openjoc-7.1.4.wav"
for path in "$PROBE_JSON" "$INSPECT_JSON" "$WAV"; do
  [[ -s "$path" ]] || fail "expected OpenJOC artifact missing or empty: $path"
done

read -r SAMPLE_RATE CHANNELS < <(python3 - "$PROBE_JSON" <<'PY'
import json, pathlib, sys
payload = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
stream = payload["streams"][0]
print(int(stream["sample_rate"]), int(stream["channels"]))
PY
)
[[ "$CHANNELS" == "12" ]] || fail "expected 12 rendered channels, got $CHANNELS"
[[ "$SAMPLE_RATE" =~ ^[1-9][0-9]*$ ]] || fail "invalid rendered sample rate: $SAMPLE_RATE"

printf '\n== Aurora temporal JOC phase: deterministic 12-channel f32 normalization ==\n'
"$FFMPEG_BIN" -hide_banner -loglevel error -y -i "$WAV" \
  -map 0:a:0 -acodec pcm_f32le -f f32le -ac 12 -ar "$SAMPLE_RATE" "$PCM"
[[ -s "$PCM" ]] || fail "normalized 12-channel f32 render is empty"

PACING_STATUS=${AURORA_JOC_PACING_STATUS:-not_evaluated}
PACING_EVIDENCE=${AURORA_JOC_PACING_EVIDENCE:-"run validation/immersive/test-joc-realtime-soak.sh separately"}
case "$PACING_STATUS" in
  pass|fail|not_evaluated) ;;
  *) fail "AURORA_JOC_PACING_STATUS must be pass, fail, or not_evaluated" ;;
esac

printf '\n== Aurora temporal JOC phase: fail-closed metadata/render diversity analysis ==\n'
set +e
python3 "$ANALYZER" analyze \
  --input "$INPUT" \
  --expected-sha256 "$EXPECTED_SHA256" \
  --provenance "$PROVENANCE" \
  --inspect "$INSPECT_JSON" \
  --pcm "$PCM" \
  --sample-rate "$SAMPLE_RATE" \
  --channels "$CHANNELS" \
  --pacing-status "$PACING_STATUS" \
  --pacing-evidence "$PACING_EVIDENCE" \
  --output "$REPORT"
ANALYZER_RC=$?
set -e

if [[ "$ANALYZER_RC" == "0" ]]; then
  echo "JOC-TEMPORAL-HARNESS-PASS"
  echo "evidence: $REPORT"
  exit 0
fi
if [[ "$ANALYZER_RC" == "3" ]]; then
  echo "JOC-TEMPORAL-HARNESS-INSUFFICIENT: carrier does not establish required temporal diversity" >&2
  echo "evidence: $REPORT" >&2
  exit 3
fi

echo "JOC-TEMPORAL-HARNESS-FAIL: analyzer rejected codec/contract/shape evidence" >&2
echo "evidence: $REPORT" >&2
exit 2
