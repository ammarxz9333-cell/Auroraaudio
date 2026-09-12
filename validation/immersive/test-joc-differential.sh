#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="$ROOT_DIR/config/external-components-v1.json"
STACK_TEST="$ROOT_DIR/validation/immersive/test-joc-stack.sh"
OPENJOC_TEST="$ROOT_DIR/validation/immersive/test-openjoc-reference.sh"
KEEP_WORKDIR=${AURORA_KEEP_JOC_DIFF_WORKDIR:-0}

fail() {
  echo "AURORA-JOC-DIFFERENTIAL-FAIL: $*" >&2
  exit 1
}

for cmd in git python3 ffmpeg ffprobe; do
  command -v "$cmd" >/dev/null 2>&1 || fail "missing required command: $cmd"
done
[[ -f "$MANIFEST" ]] || fail "missing component manifest: $MANIFEST"
[[ -f "$STACK_TEST" ]] || fail "missing validation lane: $STACK_TEST"
[[ -f "$OPENJOC_TEST" ]] || fail "missing validation lane: $OPENJOC_TEST"

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_DIR]" >&2
  exit 64
fi

USER_OUTPUT_DIR=0
if [[ $# -eq 1 ]]; then
  USER_OUTPUT_DIR=1
  WORK_DIR=$1
  rm -rf "$WORK_DIR"
  mkdir -p "$WORK_DIR"
else
  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/aurora-joc-differential.XXXXXX")"
fi

cleanup() {
  if [[ "$KEEP_WORKDIR" == "1" || "$USER_OUTPUT_DIR" == "1" ]]; then
    echo "Keeping JOC differential artifacts: $WORK_DIR"
  else
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

readarray -t HARLETTY_FIELDS < <(python3 - "$MANIFEST" <<'PY'
import json
import sys

manifest = json.load(open(sys.argv[1], encoding="utf-8"))
components = {c["id"]: c for c in manifest["components"]}
c = components["harletty-bridge"]
print(c["upstream"])
print(c["tested_version"])
print(c["pinned_commit"])
PY
)
HARLETTY_UPSTREAM=${HARLETTY_FIELDS[0]}
HARLETTY_VERSION=${HARLETTY_FIELDS[1]}
HARLETTY_COMMIT=${HARLETTY_FIELDS[2]}

FIXTURE_REPO="$WORK_DIR/fixture-source"
git clone --quiet --depth 1 --branch "$HARLETTY_VERSION" "$HARLETTY_UPSTREAM" "$FIXTURE_REPO"
ACTUAL_COMMIT=$(git -C "$FIXTURE_REPO" rev-parse HEAD)
[[ "$ACTUAL_COMMIT" == "$HARLETTY_COMMIT" ]] || fail "Harletty fixture source commit mismatch: expected $HARLETTY_COMMIT got $ACTUAL_COMMIT"

INPUT="$FIXTURE_REPO/harletty/tests/fixtures/joc_atmos_1s.eac3"
[[ -s "$INPUT" ]] || fail "pinned JOC fixture is missing: $INPUT"
INPUT_SHA256=$(python3 - "$INPUT" <<'PY'
import hashlib
import pathlib
import sys
p = pathlib.Path(sys.argv[1])
print(hashlib.sha256(p.read_bytes()).hexdigest())
PY
)
echo "DIFFERENTIAL-INPUT sha256=$INPUT_SHA256 source_commit=$ACTUAL_COMMIT"

STACK_DIR="$WORK_DIR/harletty-omniphony"
OPENJOC_DIR="$WORK_DIR/openjoc"
mkdir -p "$STACK_DIR" "$OPENJOC_DIR"

AURORA_KEEP_JOC_TEST_WORKDIR=1 \
AURORA_JOC_TEST_WORKDIR="$STACK_DIR" \
  bash "$STACK_TEST"

bash "$OPENJOC_TEST" "$INPUT" "$OPENJOC_DIR"

STACK_RAW="$STACK_DIR/joc_atmos_7_1_4.f32"
OPENJOC_WAV="$OPENJOC_DIR/openjoc-7.1.4.wav"
OPENJOC_RAW="$WORK_DIR/openjoc-7.1.4-48k.f32"
REPORT_JSON="$WORK_DIR/joc-differential-report.json"

[[ -s "$STACK_RAW" ]] || fail "Harletty/Omniphony render artifact missing: $STACK_RAW"
[[ -s "$OPENJOC_WAV" ]] || fail "OpenJOC render artifact missing: $OPENJOC_WAV"

ffmpeg -nostdin -hide_banner -loglevel error -y \
  -i "$OPENJOC_WAV" -map 0:a:0 -ar 48000 -ac 12 -c:a pcm_f32le -f f32le "$OPENJOC_RAW"
[[ -s "$OPENJOC_RAW" ]] || fail "failed to normalize OpenJOC output to 12ch/48k f32"

python3 - "$STACK_RAW" "$OPENJOC_RAW" "$REPORT_JSON" "$INPUT_SHA256" <<'PY'
from array import array
import json
import math
import pathlib
import sys

stack_path = pathlib.Path(sys.argv[1])
openjoc_path = pathlib.Path(sys.argv[2])
report_path = pathlib.Path(sys.argv[3])
input_sha256 = sys.argv[4]
channels = 12
sample_rate = 48000
active_rms_threshold = 1e-5


def read_f32(path):
    data = path.read_bytes()
    frame_bytes = channels * 4
    if not data or len(data) % frame_bytes:
        raise SystemExit(f"AURORA-JOC-DIFFERENTIAL-FAIL: invalid 12ch f32 artifact: {path} bytes={len(data)}")
    values = array("f")
    values.frombytes(data)
    if not all(math.isfinite(x) for x in values):
        raise SystemExit(f"AURORA-JOC-DIFFERENTIAL-FAIL: NaN/Inf in {path}")
    if not any(abs(x) > 1e-8 for x in values):
        raise SystemExit(f"AURORA-JOC-DIFFERENTIAL-FAIL: silent output in {path}")
    return values


def channel_stats(values):
    frames = len(values) // channels
    rms = []
    peak = []
    for ch in range(channels):
        seq = values[ch::channels]
        rms.append(math.sqrt(sum(x * x for x in seq) / len(seq)))
        peak.append(max(abs(x) for x in seq))
    return frames, rms, peak


def active_channels(rms):
    return [index for index, value in enumerate(rms) if value >= active_rms_threshold]


def correlation(a, b, ch, frames):
    if frames <= 1:
        return None
    xa = [a[i * channels + ch] for i in range(frames)]
    xb = [b[i * channels + ch] for i in range(frames)]
    ma = sum(xa) / frames
    mb = sum(xb) / frames
    va = sum((x - ma) ** 2 for x in xa)
    vb = sum((x - mb) ** 2 for x in xb)
    if va <= 1e-20 or vb <= 1e-20:
        return None
    cov = sum((xa[i] - ma) * (xb[i] - mb) for i in range(frames))
    return cov / math.sqrt(va * vb)


stack = read_f32(stack_path)
openjoc = read_f32(openjoc_path)
stack_frames, stack_rms, stack_peak = channel_stats(stack)
openjoc_frames, openjoc_rms, openjoc_peak = channel_stats(openjoc)
stack_active = active_channels(stack_rms)
openjoc_active = active_channels(openjoc_rms)
common_frames = min(stack_frames, openjoc_frames)
duration_delta = abs(stack_frames - openjoc_frames) / sample_rate

# Renderer PCM is intentionally not required to match numerically. Duration,
# layout validity, active-channel identity, and continuity are the semantic
# gates here; correlations are evidence for later investigation only.
if duration_delta > 0.25:
    raise SystemExit(
        "AURORA-JOC-DIFFERENTIAL-FAIL: programme duration divergence exceeds 250 ms: "
        f"stack_frames={stack_frames} openjoc_frames={openjoc_frames} delta_s={duration_delta:.6f}"
    )
if not stack_active or not openjoc_active:
    raise SystemExit(
        "AURORA-JOC-DIFFERENTIAL-FAIL: active-channel detection produced an empty set: "
        f"stack={stack_active} openjoc={openjoc_active} threshold={active_rms_threshold}"
    )
if stack_active != openjoc_active:
    raise SystemExit(
        "AURORA-JOC-DIFFERENTIAL-FAIL: independent renderers disagree on active channel identity: "
        f"stack={stack_active} openjoc={openjoc_active} threshold={active_rms_threshold}"
    )

correlations = [correlation(stack, openjoc, ch, common_frames) for ch in range(channels)]
report = {
    "schema_version": 1,
    "input_sha256": input_sha256,
    "sample_rate": sample_rate,
    "channels": channels,
    "active_rms_threshold": active_rms_threshold,
    "harletty_omniphony": {
        "frames": stack_frames,
        "duration_s": stack_frames / sample_rate,
        "rms": stack_rms,
        "peak": stack_peak,
        "active_channels": stack_active,
    },
    "openjoc": {
        "frames": openjoc_frames,
        "duration_s": openjoc_frames / sample_rate,
        "rms": openjoc_rms,
        "peak": openjoc_peak,
        "active_channels": openjoc_active,
    },
    "comparison": {
        "common_frames": common_frames,
        "duration_delta_s": duration_delta,
        "active_channels_match": stack_active == openjoc_active,
        "per_channel_correlation_non_gating": correlations,
    },
}
report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print(
    "JOC-DIFFERENTIAL-METRICS-PASS "
    f"stack_frames={stack_frames} openjoc_frames={openjoc_frames} "
    f"duration_delta_s={duration_delta:.6f} active_channels={stack_active}"
)
PY

echo "AURORA-JOC-DIFFERENTIAL-PASS"
echo "report: $REPORT_JSON"
