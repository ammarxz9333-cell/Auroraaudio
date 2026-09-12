#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 INPUT_JOC [OUTPUT_DIR]" >&2
  exit 64
fi

INPUT=$1
OUTPUT_DIR=${2:-"$(mktemp -d)"}
OPENJOC_BIN=${OPENJOC_BIN:-openjoc}
FFPROBE_BIN=${FFPROBE_BIN:-ffprobe}
EXPECTED_VERSION=${AURORA_OPENJOC_EXPECTED_VERSION:-0.17.0}

fail() {
  echo "OPENJOC-REFERENCE-FAIL: $*" >&2
  exit 1
}

[[ -f "$INPUT" ]] || fail "input fixture not found: $INPUT"
command -v "$OPENJOC_BIN" >/dev/null 2>&1 || fail "openjoc executable not found: $OPENJOC_BIN"
command -v "$FFPROBE_BIN" >/dev/null 2>&1 || fail "ffprobe executable not found: $FFPROBE_BIN"
command -v python3 >/dev/null 2>&1 || fail "python3 is required to validate inspector JSON"

mkdir -p "$OUTPUT_DIR"
INSPECT_JSON="$OUTPUT_DIR/openjoc-inspect.json"
OUTPUT_WAV="$OUTPUT_DIR/openjoc-7.1.4.wav"
PROBE_JSON="$OUTPUT_DIR/openjoc-7.1.4-ffprobe.json"

VERSION_TEXT=$($OPENJOC_BIN --version 2>&1) || fail "openjoc --version failed"
printf '%s\n' "$VERSION_TEXT"
[[ "$VERSION_TEXT" == *"$EXPECTED_VERSION"* ]] || fail "expected OpenJOC $EXPECTED_VERSION, got: $VERSION_TEXT"

"$OPENJOC_BIN" inspect "$INPUT" --json >"$INSPECT_JSON" || fail "OpenJOC inspect failed"
python3 - "$INSPECT_JSON" <<'PY' || exit 1
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
try:
    payload = json.loads(path.read_text(encoding="utf-8"))
except Exception as exc:
    print(f"OPENJOC-REFERENCE-FAIL: invalid inspector JSON: {exc}", file=sys.stderr)
    raise SystemExit(1)
if payload in ({}, [], None):
    print("OPENJOC-REFERENCE-FAIL: inspector JSON is empty", file=sys.stderr)
    raise SystemExit(1)
PY

echo "OPENJOC-INSPECT-PASS"

"$OPENJOC_BIN" render-joc "$INPUT" --layout 7.1.4 --output "$OUTPUT_WAV" || fail "OpenJOC 7.1.4 render failed"
[[ -s "$OUTPUT_WAV" ]] || fail "rendered WAV is missing or empty"

"$FFPROBE_BIN" -v error -select_streams a:0 -show_entries stream=codec_name,sample_rate,channels,duration -of json "$OUTPUT_WAV" >"$PROBE_JSON" || fail "ffprobe failed"

python3 - "$PROBE_JSON" <<'PY' || exit 1
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
try:
    payload = json.loads(path.read_text(encoding="utf-8"))
    stream = payload["streams"][0]
except Exception as exc:
    print(f"OPENJOC-REFERENCE-FAIL: invalid ffprobe JSON: {exc}", file=sys.stderr)
    raise SystemExit(1)
channels = int(stream.get("channels", 0))
sample_rate = int(stream.get("sample_rate", 0))
if channels != 12:
    print(f"OPENJOC-REFERENCE-FAIL: expected 12 channels for 7.1.4, got {channels}", file=sys.stderr)
    raise SystemExit(1)
if sample_rate <= 0:
    print("OPENJOC-REFERENCE-FAIL: invalid output sample rate", file=sys.stderr)
    raise SystemExit(1)
print(f"OPENJOC-7.1.4-PASS channels={channels} sample_rate={sample_rate}")
PY

echo "OPENJOC-REFERENCE-PASS"
echo "artifacts: $OUTPUT_DIR"
