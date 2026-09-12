#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EVAL_CONFIG="$ROOT_DIR/config/omniphony-evaluation-v1.json"
WORK_DIR="${AURORA_JOC_TEST_WORKDIR:-}"
TOOLCHAIN="${AURORA_EXTERNAL_RUST_TOOLCHAIN:-stable}"
BUILD_MODE="${AURORA_JOC_BUILD_MODE:-release}"
REPORT="${AURORA_OMNIP_EVAL_REPORT:-${WORK_DIR:-/tmp}/omniphony-reference-comparison.json}"
EVAL_TIMEOUT_TENTHS="${AURORA_OMNIP_EVAL_TIMEOUT_TENTHS:-300}"
EVAL_SHUTDOWN_GRACE_TENTHS="${AURORA_OMNIP_EVAL_SHUTDOWN_GRACE_TENTHS:-100}"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Omniphony reference comparison currently targets Linux." >&2
  exit 2
fi
for cmd in git python3 cargo rustup grep; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
[[ -f "$EVAL_CONFIG" ]] || { echo "missing evaluation config: $EVAL_CONFIG" >&2; exit 2; }
[[ -n "$WORK_DIR" ]] || { echo "AURORA_JOC_TEST_WORKDIR must point to a completed baseline JOC workdir" >&2; exit 2; }
[[ "$EVAL_TIMEOUT_TENTHS" =~ ^[1-9][0-9]*$ ]] || { echo "invalid AURORA_OMNIP_EVAL_TIMEOUT_TENTHS" >&2; exit 2; }
[[ "$EVAL_SHUTDOWN_GRACE_TENTHS" =~ ^[1-9][0-9]*$ ]] || { echo "invalid AURORA_OMNIP_EVAL_SHUTDOWN_GRACE_TENTHS" >&2; exit 2; }

case "$BUILD_MODE" in
  debug) PROFILE_ARGS=(); PROFILE_DIR=debug ;;
  release) PROFILE_ARGS=(--release); PROFILE_DIR=release ;;
  *) echo "unsupported AURORA_JOC_BUILD_MODE: $BUILD_MODE" >&2; exit 2 ;;
esac

STABLE_DIR="$WORK_DIR/Omniphony"
HARLETTY_DIR="$WORK_DIR/harletty-bridge"
HARLETTY_TARGET_DIR="${AURORA_HARLETTY_TARGET_DIR:-$HARLETTY_DIR/target}"
STABLE_TARGET_DIR="${AURORA_OMNIP_TARGET_DIR:-$STABLE_DIR/omniphony-renderer/target}"
EVAL_DIR="$WORK_DIR/Omniphony-evaluation"
EVAL_TARGET_DIR="${AURORA_OMNIP_EVAL_TARGET_DIR:-$WORK_DIR/omniphony-eval-target}"
IEC_FILE="$WORK_DIR/joc_atmos_1s.spdif"
STABLE_RENDER="$WORK_DIR/joc_atmos_7_1_4.f32"
BRIDGE_LIB="$HARLETTY_TARGET_DIR/$PROFILE_DIR/libharletty_bridge.so"
STABLE_OR="${STABLE_TARGET_DIR}/$PROFILE_DIR/orender"
STABLE_LAYOUT="$STABLE_DIR/layouts/7.1.4.yaml"
EVAL_RENDER="$WORK_DIR/joc_atmos_7_1_4-evaluation.f32"
EVAL_LOG="$WORK_DIR/orender-joc-evaluation.log"
EVAL_PID=""

cleanup_candidate() {
  if [[ -n "$EVAL_PID" ]] && kill -0 "$EVAL_PID" 2>/dev/null; then
    kill -TERM "$EVAL_PID" 2>/dev/null || true
    sleep 0.2
    kill -KILL "$EVAL_PID" 2>/dev/null || true
    wait "$EVAL_PID" 2>/dev/null || true
  fi
}
trap cleanup_candidate EXIT

for path in "$STABLE_DIR/omniphony-renderer/bridge_api/src/lib.rs" "$STABLE_DIR/omniphony-renderer/bridge_api/src/labels.rs" "$STABLE_LAYOUT" "$IEC_FILE" "$STABLE_RENDER" "$BRIDGE_LIB" "$STABLE_OR"; do
  [[ -f "$path" ]] || { echo "missing baseline artifact: $path" >&2; exit 1; }
done

eval "$(python3 - "$EVAL_CONFIG" <<'PY'
import json, shlex, sys
cfg = json.load(open(sys.argv[1], encoding='utf-8'))
for key, value in [
    ('UPSTREAM', cfg['upstream']),
    ('STABLE_COMMIT', cfg['stable']['commit']),
    ('STABLE_LABEL', cfg['stable']['label']),
    ('EVAL_COMMIT', cfg['candidate']['commit']),
    ('EVAL_LABEL', cfg['candidate']['label']),
]:
    print(f"{key}={shlex.quote(value)}")
PY
)"

ACTUAL_STABLE="$(git -C "$STABLE_DIR" rev-parse HEAD)"
if [[ "$ACTUAL_STABLE" != "$STABLE_COMMIT" ]]; then
  echo "stable checkout mismatch: expected $STABLE_COMMIT got $ACTUAL_STABLE" >&2
  exit 1
fi

rm -rf "$EVAL_DIR"
mkdir -p "$EVAL_DIR" "$EVAL_TARGET_DIR" "$(dirname "$REPORT")"
git -C "$EVAL_DIR" init --quiet
git -C "$EVAL_DIR" remote add origin "$UPSTREAM"
git -C "$EVAL_DIR" fetch --quiet --depth 1 origin "$EVAL_COMMIT"
git -C "$EVAL_DIR" checkout --quiet --detach FETCH_HEAD
ACTUAL_EVAL="$(git -C "$EVAL_DIR" rev-parse HEAD)"
if [[ "$ACTUAL_EVAL" != "$EVAL_COMMIT" ]]; then
  echo "evaluation checkout mismatch: expected $EVAL_COMMIT got $ACTUAL_EVAL" >&2
  exit 1
fi

EVAL_LAYOUT="$EVAL_DIR/layouts/7.1.4.yaml"
EVAL_BRIDGE_API="$EVAL_DIR/omniphony-renderer/bridge_api/src/lib.rs"
EVAL_LABELS="$EVAL_DIR/omniphony-renderer/bridge_api/src/labels.rs"
for path in "$EVAL_LAYOUT" "$EVAL_BRIDGE_API" "$EVAL_LABELS"; do
  [[ -f "$path" ]] || { echo "evaluation source missing: $path" >&2; exit 1; }
done

STABLE_BRIDGE_SHA="$(git -C "$STABLE_DIR" hash-object omniphony-renderer/bridge_api/src/lib.rs)"
EVAL_BRIDGE_SHA="$(git -C "$EVAL_DIR" hash-object omniphony-renderer/bridge_api/src/lib.rs)"
if [[ "$STABLE_BRIDGE_SHA" != "$EVAL_BRIDGE_SHA" ]]; then
  echo "bridge ABI source drift detected: stable=$STABLE_BRIDGE_SHA candidate=$EVAL_BRIDGE_SHA" >&2
  exit 1
fi
STABLE_LAYOUT_SHA="$(git -C "$STABLE_DIR" hash-object layouts/7.1.4.yaml)"
EVAL_LAYOUT_SHA="$(git -C "$EVAL_DIR" hash-object layouts/7.1.4.yaml)"
if [[ "$STABLE_LAYOUT_SHA" != "$EVAL_LAYOUT_SHA" ]]; then
  echo "7.1.4 layout drift detected: stable=$STABLE_LAYOUT_SHA candidate=$EVAL_LAYOUT_SHA" >&2
  exit 1
fi

LABEL_REPORT="$WORK_DIR/omniphony-label-contract.json"
python3 - "$STABLE_DIR/omniphony-renderer/bridge_api/src/lib.rs" "$EVAL_BRIDGE_API" "$STABLE_DIR/omniphony-renderer/bridge_api/src/labels.rs" "$EVAL_LABELS" "$LABEL_REPORT" <<'PY'
import hashlib, json, pathlib, re, sys
stable_lib, eval_lib, stable_labels, eval_labels, report = map(pathlib.Path, sys.argv[1:])

def enum_values(text):
    m = re.search(r"pub enum RChannelLabel\s*\{(.*?)\n\}", text, re.S)
    if not m:
        raise SystemExit("RChannelLabel enum not found")
    return {
        name: int(value)
        for name, value in re.findall(r"^\s*([A-Za-z0-9_]+)\s*=\s*([0-9]+)\s*,", m.group(1), re.M)
    }

def canonical(text):
    m = re.search(r"pub fn canonical_name\(.*?\)\s*->\s*&'static str\s*\{(.*?)\n\}", text, re.S)
    if not m:
        raise SystemExit("canonical_name function not found")
    return dict(re.findall(r"^\s*([A-Za-z0-9_]+)\s*=>\s*\"([^\"]+)\"", m.group(1), re.M))

slib = stable_lib.read_text(encoding='utf-8')
elib = eval_lib.read_text(encoding='utf-8')
slab = stable_labels.read_text(encoding='utf-8')
elab = eval_labels.read_text(encoding='utf-8')
sev, eev = enum_values(slib), enum_values(elib)
sc, ec = canonical(slab), canonical(elab)
if sev != eev:
    raise SystemExit(f"RChannelLabel discriminants changed: stable={sev} candidate={eev}")
if sc != ec:
    raise SystemExit(f"canonical channel names changed: stable={sc} candidate={ec}")
payload = {
    'enum_discriminants_equal': True,
    'canonical_names_equal': True,
    'stable_labels_sha256': hashlib.sha256(slab.encode()).hexdigest(),
    'candidate_labels_sha256': hashlib.sha256(elab.encode()).hexdigest(),
    'labels_source_identical': slab == elab,
}
report.write_text(json.dumps(payload, indent=2, sort_keys=True) + '\n', encoding='utf-8')
print('OMNIP-COMPAT-LABELS-PASS')
PY

rustup toolchain install "$TOOLCHAIN" --profile minimal >/dev/null
START_NS="$(python3 - <<'PY'
import time
print(time.monotonic_ns())
PY
)"
CARGO_TARGET_DIR="$EVAL_TARGET_DIR" cargo +"$TOOLCHAIN" build "${PROFILE_ARGS[@]}" \
  --manifest-path "$EVAL_DIR/omniphony-renderer/Cargo.toml" \
  -p omniphony-renderer
BUILD_END_NS="$(python3 - <<'PY'
import time
print(time.monotonic_ns())
PY
)"
EVAL_OR="$EVAL_TARGET_DIR/$PROFILE_DIR/orender"
[[ -f "$EVAL_OR" ]] || { echo "evaluation orender missing: $EVAL_OR" >&2; exit 1; }

rm -f "$EVAL_RENDER" "$EVAL_LOG"
RENDER_START_NS="$(python3 - <<'PY'
import time
print(time.monotonic_ns())
PY
)"
RUST_LOG="${RUST_LOG:-info}" "$EVAL_OR" "$IEC_FILE" \
  --bridge-path "$BRIDGE_LIB" \
  --enable-vbap \
  --speaker-layout "$EVAL_LAYOUT" \
  --output-backend file \
  --output-file "$EVAL_RENDER" \
  --output-file-format raw-f32 \
  --continuous \
  >"$EVAL_LOG" 2>&1 &
EVAL_PID=$!

# Newer Omniphony revisions intentionally keep the process alive after a file
# StreamEnd so Studio/live input can hand over another stream. For this one-shot
# evaluation, wait for the explicit post-finalize marker and then request a
# normal SIGTERM shutdown. A future revision that exits naturally is accepted
# too. Timeout, forced kill, non-zero exit, or incomplete output all fail closed.
STREAM_END_MARKER="Handler reset complete, ready for next stream"
EVAL_STREAM_END_OBSERVED=false
EVAL_TERMINATION_MODE=""
EVAL_EXIT_STATUS=0

for ((i=0; i<EVAL_TIMEOUT_TENTHS; i++)); do
  if ! kill -0 "$EVAL_PID" 2>/dev/null; then
    set +e
    wait "$EVAL_PID"
    EVAL_EXIT_STATUS=$?
    set -e
    EVAL_PID=""
    if [[ "$EVAL_EXIT_STATUS" -ne 0 ]]; then
      echo "evaluation Omniphony exited with status $EVAL_EXIT_STATUS" >&2
      cat "$EVAL_LOG" >&2 || true
      exit 1
    fi
    EVAL_TERMINATION_MODE="natural_exit"
    break
  fi

  if grep -Fq "$STREAM_END_MARKER" "$EVAL_LOG" 2>/dev/null; then
    EVAL_STREAM_END_OBSERVED=true
    EVAL_TERMINATION_MODE="graceful_shutdown_after_stream_end"
    kill -TERM "$EVAL_PID"
    for ((j=0; j<EVAL_SHUTDOWN_GRACE_TENTHS; j++)); do
      if ! kill -0 "$EVAL_PID" 2>/dev/null; then
        break
      fi
      sleep 0.1
    done
    if kill -0 "$EVAL_PID" 2>/dev/null; then
      echo "evaluation Omniphony did not stop within the shutdown grace period" >&2
      kill -KILL "$EVAL_PID" 2>/dev/null || true
      wait "$EVAL_PID" 2>/dev/null || true
      EVAL_PID=""
      cat "$EVAL_LOG" >&2 || true
      exit 1
    fi
    set +e
    wait "$EVAL_PID"
    EVAL_EXIT_STATUS=$?
    set -e
    EVAL_PID=""
    if [[ "$EVAL_EXIT_STATUS" -ne 0 ]]; then
      echo "evaluation Omniphony shutdown returned status $EVAL_EXIT_STATUS" >&2
      cat "$EVAL_LOG" >&2 || true
      exit 1
    fi
    break
  fi
  sleep 0.1
done

if [[ -z "$EVAL_TERMINATION_MODE" ]]; then
  echo "evaluation Omniphony did not finish one stream within $((EVAL_TIMEOUT_TENTHS / 10)) seconds" >&2
  cleanup_candidate
  EVAL_PID=""
  cat "$EVAL_LOG" >&2 || true
  exit 1
fi
[[ -s "$EVAL_RENDER" ]] || { echo "evaluation Omniphony produced no rendered output" >&2; cat "$EVAL_LOG" >&2 || true; exit 1; }

RENDER_END_NS="$(python3 - <<'PY'
import time
print(time.monotonic_ns())
PY
)"

python3 - "$EVAL_CONFIG" "$LABEL_REPORT" "$STABLE_RENDER" "$EVAL_RENDER" "$REPORT" "$STABLE_BRIDGE_SHA" "$EVAL_BRIDGE_SHA" "$STABLE_LAYOUT_SHA" "$EVAL_LAYOUT_SHA" "$START_NS" "$BUILD_END_NS" "$RENDER_START_NS" "$RENDER_END_NS" "$EVAL_TERMINATION_MODE" "$EVAL_STREAM_END_OBSERVED" <<'PY'
from array import array
import hashlib, json, math, pathlib, sys
(
    cfg_path, label_path, stable_path, candidate_path, report_path,
    stable_bridge_sha, candidate_bridge_sha, stable_layout_sha, candidate_layout_sha,
    build_start_ns, build_end_ns, render_start_ns, render_end_ns,
    termination_mode, stream_end_observed,
) = sys.argv[1:]
cfg = json.loads(pathlib.Path(cfg_path).read_text(encoding='utf-8'))
labels = json.loads(pathlib.Path(label_path).read_text(encoding='utf-8'))

def metrics(path):
    p = pathlib.Path(path)
    data = p.read_bytes()
    frame_bytes = 12 * 4
    if not data or len(data) % frame_bytes:
        raise SystemExit(f"invalid 12-channel raw-f32 output: {p} bytes={len(data)}")
    samples = array('f')
    samples.frombytes(data)
    if not all(math.isfinite(v) for v in samples):
        raise SystemExit(f"NaN/Inf in {p}")
    frames = len(samples) // 12
    rms, peak = [], []
    for ch in range(12):
        vals = samples[ch::12]
        rms.append(math.sqrt(sum(v*v for v in vals) / len(vals)))
        peak.append(max(abs(v) for v in vals))
    if max(peak) <= 1e-8:
        raise SystemExit(f"silent output: {p}")
    return {
        'sha256': hashlib.sha256(data).hexdigest(),
        'bytes': len(data),
        'channels': 12,
        'sample_rate_hz': 48000,
        'frames': frames,
        'duration_seconds': frames / 48000.0,
        'rms': rms,
        'peak': peak,
        'active_channels_rms_gt_1e-8': sum(v > 1e-8 for v in rms),
        'finite': True,
        'non_silent': True,
    }

stable = metrics(stable_path)
candidate = metrics(candidate_path)
if stable['frames'] != candidate['frames']:
    raise SystemExit(f"frame-count drift: stable={stable['frames']} candidate={candidate['frames']}")
if candidate['active_channels_rms_gt_1e-8'] < stable['active_channels_rms_gt_1e-8']:
    raise SystemExit(
        "candidate activates fewer output lanes than stable: "
        f"stable={stable['active_channels_rms_gt_1e-8']} candidate={candidate['active_channels_rms_gt_1e-8']}"
    )

delta = {
    'rms_absolute': [c-s for s,c in zip(stable['rms'], candidate['rms'])],
    'peak_absolute': [c-s for s,c in zip(stable['peak'], candidate['peak'])],
    'byte_identical': stable['sha256'] == candidate['sha256'],
}
report = {
    'schema_version': 1,
    'verdict': 'pass',
    'truth_boundary': 'Evaluation-only external reference evidence. A pass does not promote the candidate pin and does not prove authored spatial correctness, hardware behavior, Dolby certification, or proprietary streaming compatibility.',
    'pins': {'stable': cfg['stable'], 'candidate': cfg['candidate'], 'upstream': cfg['upstream']},
    'compatibility': {
        'bridge_api_source_identical': stable_bridge_sha == candidate_bridge_sha,
        'bridge_api_blob_sha': stable_bridge_sha,
        'layout_source_identical': stable_layout_sha == candidate_layout_sha,
        'layout_blob_sha': stable_layout_sha,
        'channel_labels': labels,
    },
    'candidate_lifecycle': {
        'one_shot_intent': True,
        'stream_end_marker_observed': stream_end_observed == 'true',
        'termination_mode': termination_mode,
        'forced_kill_accepted': False,
    },
    'stable_render': stable,
    'candidate_render': candidate,
    'delta': delta,
    'timing_diagnostics': {
        'candidate_build_wall_seconds': (int(build_end_ns)-int(build_start_ns))/1e9,
        'candidate_render_wall_seconds': (int(render_end_ns)-int(render_start_ns))/1e9,
        'note': 'CI wall-clock diagnostics are informational and are not a realtime latency measurement.'
    },
    'promotion': {'performed': False, 'stable_pin_unchanged': True},
}
pathlib.Path(report_path).write_text(json.dumps(report, indent=2, sort_keys=True) + '\n', encoding='utf-8')
print(
    'OMNIP-REFERENCE-COMPARISON-PASS '
    f"frames={stable['frames']} stable_active={stable['active_channels_rms_gt_1e-8']} "
    f"candidate_active={candidate['active_channels_rms_gt_1e-8']} byte_identical={delta['byte_identical']} "
    f"termination={termination_mode}"
)
PY

cat "$REPORT"
