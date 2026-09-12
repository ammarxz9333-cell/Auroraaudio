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

"$OPENJOC_BIN" inspect "$INPUT" --json --objects --emdf >"$INSPECT_JSON" || fail "OpenJOC inspect failed"
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

try:
    joc = payload["joc"]
    eac3 = payload["eac3"]
    validation = payload["validation"]
    diagnostics = payload["diagnostics"]
except Exception as exc:
    print(f"OPENJOC-REFERENCE-FAIL: inspector JSON missing required contract fields: {exc}", file=sys.stderr)
    raise SystemExit(1)

if joc.get("present") is not True:
    print("OPENJOC-REFERENCE-FAIL: input was not positively identified as JOC", file=sys.stderr)
    raise SystemExit(1)
if int(eac3.get("access_unit_count", 0)) <= 0:
    print("OPENJOC-REFERENCE-FAIL: inspector reported no complete E-AC-3 access units", file=sys.stderr)
    raise SystemExit(1)
if int(eac3.get("total_samples", 0)) <= 0:
    print("OPENJOC-REFERENCE-FAIL: inspector reported no programme samples", file=sys.stderr)
    raise SystemExit(1)
if validation.get("stream_parse") != "pass":
    print(
        f"OPENJOC-REFERENCE-FAIL: stream_parse={validation.get('stream_parse')!r}",
        file=sys.stderr,
    )
    raise SystemExit(1)
if validation.get("decoder_admissible") is not True:
    print("OPENJOC-REFERENCE-FAIL: stream was not admitted by the decoder contract", file=sys.stderr)
    raise SystemExit(1)
if validation.get("frame_timing_continuity") != "continuous":
    print(
        "OPENJOC-REFERENCE-FAIL: frame timing is not continuous: "
        f"{validation.get('frame_timing_continuity')!r}",
        file=sys.stderr,
    )
    raise SystemExit(1)
if validation.get("metadata_timing_continuity") != "continuous":
    print(
        "OPENJOC-REFERENCE-FAIL: metadata timing is not continuous: "
        f"{validation.get('metadata_timing_continuity')!r}",
        file=sys.stderr,
    )
    raise SystemExit(1)
compatibility = validation.get("deployed_compatibility") or {}
if compatibility.get("status") != "pass":
    print(
        "OPENJOC-REFERENCE-FAIL: deployed compatibility gate did not pass: "
        f"{compatibility.get('status')!r}",
        file=sys.stderr,
    )
    raise SystemExit(1)
if diagnostics.get("complete") is not True or int(diagnostics.get("issue_count", 0)) != 0:
    print(
        "OPENJOC-REFERENCE-FAIL: inspection diagnostics were incomplete or reported issues",
        file=sys.stderr,
    )
    raise SystemExit(1)
profiles = joc.get("profiles") or []
if not profiles:
    print("OPENJOC-REFERENCE-FAIL: JOC was signaled but no profile was reported", file=sys.stderr)
    raise SystemExit(1)

strict = validation.get("etsi_strict") or {}
print(
    "OPENJOC-INSPECT-PASS "
    f"access_units={eac3['access_unit_count']} samples={eac3['total_samples']} "
    f"profiles={len(profiles)} presence_status={joc.get('presence_status', 'unknown')} "
    f"decoder_admissible=true timing=continuous "
    f"deployed_compatibility=pass etsi_strict={strict.get('status', 'unknown')}"
)
PY

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
try:
    duration = float(stream.get("duration", 0) or 0)
except (TypeError, ValueError):
    duration = 0.0
if channels != 12:
    print(f"OPENJOC-REFERENCE-FAIL: expected 12 channels for 7.1.4, got {channels}", file=sys.stderr)
    raise SystemExit(1)
if sample_rate <= 0:
    print("OPENJOC-REFERENCE-FAIL: invalid output sample rate", file=sys.stderr)
    raise SystemExit(1)
if duration <= 0:
    print("OPENJOC-REFERENCE-FAIL: rendered output has no positive duration", file=sys.stderr)
    raise SystemExit(1)
print(
    f"OPENJOC-7.1.4-PASS channels={channels} sample_rate={sample_rate} "
    f"duration={duration:.6f} codec={stream.get('codec_name', 'unknown')}"
)
PY

echo "OPENJOC-REFERENCE-PASS"
echo "artifacts: $OUTPUT_DIR"
