#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BASELINE="$ROOT_DIR/validation/immersive/test-joc-stack.sh"
LOOPS="${AURORA_JOC_REALTIME_LOOPS:-4}"
BUILD_MODE="${AURORA_JOC_BUILD_MODE:-debug}"
TOOLCHAIN="${AURORA_EXTERNAL_RUST_TOOLCHAIN:-stable}"
KEEP_WORKDIR="${AURORA_KEEP_JOC_REALTIME_WORKDIR:-0}"

case "$LOOPS" in
  ''|*[!0-9]*)
    echo "AURORA_JOC_REALTIME_LOOPS must be a positive integer" >&2
    exit 2
    ;;
esac
if (( LOOPS < 1 )); then
  echo "AURORA_JOC_REALTIME_LOOPS must be at least 1" >&2
  exit 2
fi

case "$BUILD_MODE" in
  debug)
    PROFILE_ARGS=()
    PROFILE_DIR="debug"
    ;;
  release)
    PROFILE_ARGS=(--release)
    PROFILE_DIR="release"
    ;;
  *)
    echo "unsupported AURORA_JOC_BUILD_MODE: $BUILD_MODE (expected debug or release)" >&2
    exit 2
    ;;
esac

for cmd in python3 cargo timeout; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
[[ -x "$BASELINE" || -f "$BASELINE" ]] || { echo "missing baseline JOC validation: $BASELINE" >&2; exit 2; }

OWN_WORKDIR=0
if [[ -n "${AURORA_JOC_TEST_WORKDIR:-}" ]]; then
  WORK_DIR="$AURORA_JOC_TEST_WORKDIR"
else
  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/aurora-joc-realtime.XXXXXX")"
  OWN_WORKDIR=1
fi

cleanup() {
  if [[ "$KEEP_WORKDIR" == "1" ]]; then
    echo "Keeping realtime JOC validation workdir: $WORK_DIR"
  elif (( OWN_WORKDIR == 1 )); then
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

export AURORA_JOC_TEST_WORKDIR="$WORK_DIR"
export AURORA_KEEP_JOC_TEST_WORKDIR=1

printf '\n== Aurora realtime JOC phase: baseline software proof ==\n'
bash "$BASELINE"

HARLETTY_DIR="$WORK_DIR/harletty-bridge"
OMNIP_DIR="$WORK_DIR/Omniphony"
HARLETTY_TARGET_DIR="${AURORA_HARLETTY_TARGET_DIR:-$HARLETTY_DIR/target}"
OMNIP_TARGET_DIR="${AURORA_OMNIP_TARGET_DIR:-$OMNIP_DIR/omniphony-renderer/target}"
HARNESS_TARGET_DIR="${AURORA_JOC_HARNESS_TARGET_DIR:-$WORK_DIR/iec-joc-harness-target}"
HARNESS_DIR="$WORK_DIR/iec-joc-harness"
BASE_IEC="$WORK_DIR/joc_atmos_1s.spdif"
BASE_RENDER="$WORK_DIR/joc_atmos_aurora_11_1_4.f32"
SOAK_IEC="$WORK_DIR/joc_atmos_continuous.spdif"
SOAK_RENDER="$WORK_DIR/joc_atmos_paced_aurora_11_1_4.f32"
BRIDGE_LIB="$HARLETTY_TARGET_DIR/$PROFILE_DIR/libharletty_bridge.so"
ORENDER="$OMNIP_TARGET_DIR/$PROFILE_DIR/orender"
LAYOUT="$ROOT_DIR/config/layouts/omniphony-11.1.4-aurora.yaml"
BRIDGE_LOG="$WORK_DIR/joc-continuous-bridge.log"
PACED_LOG="$WORK_DIR/orender-joc-paced.log"

for path in "$BASE_IEC" "$BASE_RENDER" "$BRIDGE_LIB" "$ORENDER" "$LAYOUT" "$HARNESS_DIR/Cargo.toml"; do
  [[ -f "$path" ]] || { echo "expected realtime validation input/build product missing: $path" >&2; exit 1; }
done

printf '\n== Aurora realtime JOC phase: prepare repeated IEC61937 carrier ==\n'
python3 - "$BASE_IEC" "$BASE_RENDER" "$SOAK_IEC" "$LOOPS" <<'PY'
import pathlib, sys

carrier_path = pathlib.Path(sys.argv[1])
render_path = pathlib.Path(sys.argv[2])
out_path = pathlib.Path(sys.argv[3])
loops = int(sys.argv[4])
carrier = carrier_path.read_bytes()
render = render_path.read_bytes()
sync = bytes.fromhex("72f81f4e")
if not carrier.startswith(sync):
    raise SystemExit("baseline IEC61937 carrier does not start with sync preamble")
second = carrier.find(sync, 4)
if second <= 0:
    raise SystemExit("baseline IEC61937 carrier does not contain multiple bursts")
burst_bytes = second
if len(carrier) % burst_bytes:
    raise SystemExit(f"carrier length {len(carrier)} is not a multiple of burst size {burst_bytes}")
base_bursts = len(carrier) // burst_bytes
for index in range(base_bursts):
    offset = index * burst_bytes
    if carrier[offset:offset + 4] != sync:
        raise SystemExit(f"missing IEC61937 sync at burst {index} offset {offset}")
frame_bytes = 16 * 4
if len(render) % frame_bytes:
    raise SystemExit("baseline Aurora 11.1.4 render is not whole 12-channel f32 frames")
base_frames = len(render) // frame_bytes
if base_frames == 0 or base_frames % base_bursts:
    raise SystemExit(
        f"cannot derive a stable media cadence: frames={base_frames} bursts={base_bursts}"
    )
frames_per_burst = base_frames // base_bursts
out_path.write_bytes(carrier * loops)
print(
    "JOC-REALTIME-CARRIER-PREP "
    f"loops={loops} base_bursts={base_bursts} burst_bytes={burst_bytes} "
    f"frames_per_burst={frames_per_burst} total_bursts={base_bursts * loops}"
)
PY

printf '\n== Aurora realtime JOC phase: continuous bridge/object metadata soak ==\n'
CARGO_TARGET_DIR="$HARNESS_TARGET_DIR" cargo +"$TOOLCHAIN" run --quiet "${PROFILE_ARGS[@]}" \
  --manifest-path "$HARNESS_DIR/Cargo.toml" -- \
  "$BRIDGE_LIB" "$SOAK_IEC" objects | tee "$BRIDGE_LOG"

python3 - "$BASE_IEC" "$BRIDGE_LOG" "$LOOPS" <<'PY'
import pathlib, re, sys

carrier = pathlib.Path(sys.argv[1]).read_bytes()
log = pathlib.Path(sys.argv[2]).read_text(encoding="utf-8")
loops = int(sys.argv[3])
sync = bytes.fromhex("72f81f4e")
second = carrier.find(sync, 4)
if second <= 0 or len(carrier) % second:
    raise SystemExit("cannot derive baseline IEC61937 burst count")
expected_packets = (len(carrier) // second) * loops
match = re.search(
    r"JOC-IEC61937-PASS packets=(\d+) frames=(\d+) metadata_frames=(\d+) events=(\d+) object_channels=(\d+)",
    log,
)
if not match:
    raise SystemExit("continuous bridge run did not emit JOC-IEC61937-PASS")
packets, frames, metadata_frames, events, object_channels = map(int, match.groups())
if packets != expected_packets:
    raise SystemExit(f"continuous bridge packet mismatch: expected {expected_packets}, got {packets}")
if frames != packets:
    raise SystemExit(f"continuous bridge frame mismatch: packets={packets} frames={frames}")
if metadata_frames != frames:
    raise SystemExit(
        f"continuous bridge metadata coverage mismatch: frames={frames} metadata_frames={metadata_frames}"
    )
if events <= 0 or object_channels <= 0:
    raise SystemExit("continuous bridge lost JOC object metadata")
print(
    "JOC-CONTINUOUS-BRIDGE-PASS "
    f"loops={loops} packets={packets} frames={frames} metadata_frames={metadata_frames} "
    f"events={events} object_channels={object_channels}"
)
PY

TIMEOUT_SECONDS="${AURORA_JOC_REALTIME_TIMEOUT_SECONDS:-$((LOOPS * 3 + 20))}"
case "$TIMEOUT_SECONDS" in
  ''|*[!0-9]*)
    echo "AURORA_JOC_REALTIME_TIMEOUT_SECONDS must be a positive integer" >&2
    exit 2
    ;;
esac
if (( TIMEOUT_SECONDS < 1 )); then
  echo "AURORA_JOC_REALTIME_TIMEOUT_SECONDS must be at least 1" >&2
  exit 2
fi

printf '\n== Aurora realtime JOC phase: media-paced stdin render to Aurora 11.1.4 ==\n'
START_NS="$(python3 - <<'PY'
import time
print(time.monotonic_ns())
PY
)"
set +e
python3 - "$BASE_IEC" "$BASE_RENDER" "$LOOPS" <<'PY' | \
  RUST_LOG="${RUST_LOG:-info}" timeout --signal=TERM --kill-after=5s "${TIMEOUT_SECONDS}s" \
  "$ORENDER" - \
    --bridge-path "$BRIDGE_LIB" \
    --enable-vbap \
    --speaker-layout "$LAYOUT" \
    --output-backend file \
    --output-file "$SOAK_RENDER" \
    --output-file-format raw-f32 \
    >"$PACED_LOG" 2>&1
import pathlib, sys, time

carrier = pathlib.Path(sys.argv[1]).read_bytes()
render = pathlib.Path(sys.argv[2]).read_bytes()
loops = int(sys.argv[3])
sync = bytes.fromhex("72f81f4e")
if not carrier.startswith(sync):
    raise SystemExit("carrier sync missing")
second = carrier.find(sync, 4)
if second <= 0 or len(carrier) % second:
    raise SystemExit("cannot derive fixed IEC61937 burst size")
burst_bytes = second
base_bursts = len(carrier) // burst_bytes
frame_bytes = 16 * 4
if len(render) % frame_bytes:
    raise SystemExit("baseline render shape invalid")
base_frames = len(render) // frame_bytes
if base_frames % base_bursts:
    raise SystemExit("baseline render cadence is not integral per IEC61937 burst")
frames_per_burst = base_frames // base_bursts
interval = frames_per_burst / 48_000.0
out = sys.stdout.buffer
deadline = time.monotonic()
for _ in range(loops):
    for burst_index in range(base_bursts):
        offset = burst_index * burst_bytes
        burst = carrier[offset:offset + burst_bytes]
        if burst[:4] != sync:
            raise SystemExit(f"sync lost at burst {burst_index}")
        out.write(burst)
        out.flush()
        deadline += interval
        remaining = deadline - time.monotonic()
        if remaining > 0:
            time.sleep(remaining)
PY
PIPE_RC=("${PIPESTATUS[@]}")
set -e
END_NS="$(python3 - <<'PY'
import time
print(time.monotonic_ns())
PY
)"
FEED_RC="${PIPE_RC[0]:-1}"
RENDER_RC="${PIPE_RC[1]:-1}"
if [[ "$FEED_RC" != "0" || "$RENDER_RC" != "0" ]]; then
  echo "paced JOC stream failed: feeder_rc=$FEED_RC renderer_rc=$RENDER_RC" >&2
  cat "$PACED_LOG" >&2 || true
  exit 1
fi

python3 - "$BASE_IEC" "$BASE_RENDER" "$SOAK_RENDER" "$LOOPS" "$START_NS" "$END_NS" <<'PY'
from array import array
import math, pathlib, sys

carrier = pathlib.Path(sys.argv[1]).read_bytes()
base_render = pathlib.Path(sys.argv[2]).read_bytes()
soak_render = pathlib.Path(sys.argv[3]).read_bytes()
loops = int(sys.argv[4])
start_ns = int(sys.argv[5])
end_ns = int(sys.argv[6])
sync = bytes.fromhex("72f81f4e")
second = carrier.find(sync, 4)
if second <= 0 or len(carrier) % second:
    raise SystemExit("cannot derive baseline burst geometry")
base_bursts = len(carrier) // second
frame_bytes = 16 * 4
if len(base_render) % frame_bytes or len(soak_render) % frame_bytes:
    raise SystemExit("Aurora 11.1.4 output is not whole 12-channel f32 frames")
base_frames = len(base_render) // frame_bytes
actual_frames = len(soak_render) // frame_bytes
expected_frames = base_frames * loops
if actual_frames != expected_frames:
    raise SystemExit(
        f"paced render frame mismatch: expected {expected_frames}, got {actual_frames}"
    )
samples = array('f')
samples.frombytes(soak_render)
if not samples or not all(math.isfinite(v) for v in samples):
    raise SystemExit("paced render contains no samples or NaN/Inf")
if not any(abs(v) > 1e-8 for v in samples):
    raise SystemExit("paced render is silent")
elapsed = (end_ns - start_ns) / 1_000_000_000.0
media_seconds = expected_frames / 48_000.0
minimum_elapsed = media_seconds * 0.85
maximum_elapsed = media_seconds * 1.35 + 1.5
if elapsed < minimum_elapsed:
    raise SystemExit(
        f"paced render completed implausibly early: media={media_seconds:.3f}s elapsed={elapsed:.3f}s"
    )
if elapsed > maximum_elapsed:
    raise SystemExit(
        f"renderer did not keep up with paced input: media={media_seconds:.3f}s elapsed={elapsed:.3f}s "
        f"limit={maximum_elapsed:.3f}s"
    )
realtime_factor = media_seconds / elapsed if elapsed > 0 else 0.0
print(
    "JOC-PACED-AURORA-11.1.4-PASS "
    f"loops={loops} bursts={base_bursts * loops} frames={actual_frames} "
    f"media_seconds={media_seconds:.3f} elapsed_seconds={elapsed:.3f} "
    f"realtime_factor={realtime_factor:.3f}"
)
PY

echo "AURORA JOC PACED STREAM PASS"
