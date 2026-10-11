#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="${AURORA_EXTERNAL_COMPONENT_MANIFEST:-$ROOT_DIR/config/external-components-v1.json}"
HARNESS_SOURCE="$ROOT_DIR/validation/physical/aurora_live_earc_harness.rs"
PIPE_ADAPTER="$ROOT_DIR/validation/physical/aurora_alsa_iec61937_pipe.py"
BED_SOURCE="$ROOT_DIR/validation/streaming/aurora_streaming_bed.py"
AUTO_BED_SOURCE="$ROOT_DIR/validation/streaming/aurora_auto_bed.py"
ADAPTIVE_SOURCE="$ROOT_DIR/validation/physical/aurora_adaptive_output.rs"
UMC_PREFLIGHT="$ROOT_DIR/validation/physical/aurora_umc1820_preflight.py"
TOOLCHAIN="${AURORA_EXTERNAL_RUST_TOOLCHAIN:-stable}"
BUILD_MODE="${AURORA_JOC_BUILD_MODE:-release}"
KEEP_WORKDIR="${AURORA_KEEP_LIVE_EARC_WORKDIR:-0}"

MODE=""
CAPTURE_DEVICE=""
OUTPUT_FILE=""
OUTPUT_DEVICE=""
OUTPUT_PROFILE="native"
DECODER_MODE="joc-native"
REQUIRE_SURROUND_INPUT=0
UMC_CARD="UMC1820"
MAX_SECONDS=""
WORK_DIR=""

usage() {
  cat <<'EOF'
Usage:
  validation/physical/run-live-earc-vbap.sh --fixture [--work-dir DIR]
  validation/physical/run-live-earc-vbap.sh --capture-device DEV --output-file FILE [--decoder-mode joc-native|channel-bed|auto-bed|auto-bed] [--seconds N] [--work-dir DIR]
  validation/physical/run-live-earc-vbap.sh --capture-device DEV --output-device DEV [--decoder-mode joc-native|channel-bed] [--output-profile native|umc1820-ada8200] [--umc-card CARD] [--seconds N] [--work-dir DIR]

Physical mode:
  TV/eARC -> ALSA S32_LE/2ch/192k -> IEC61937 -> selected decoder:
     joc-native: E-AC-3 JOC -> Harletty object renderer
     channel-bed: AC-3 or DD+ 2/5.1/7.1 -> FFmpeg bed + synthetic ambience
  -> canonical S32_LE/12ch/48k -> file or one ALSA output device.
  Stereo input is explicitly upmixed/labeled unless --require-surround-input is set.
  auto-bed: validated AC-3 <-> DD+ channel-bed codec switching (no JOC objects).
  Only AC-3/DD+ bed changes auto-switch in this mode; JOC needs native decoder selection.

Direct ALSA-device mode inserts Aurora's bounded adaptive duplex bridge between
VBAP and aplay: one 12-channel FIFO, PI drift controller, clock estimator, and
Rubato ASRC shared coherently across all logical channels. The optional
umc1820-ada8200 profile expands the 12 logical channels to the UMC1820's 20 USB
playback slots, routing Aurora to Line Out 3-10 and ADAT 1-4 while silencing the
unused Main, S/PDIF, and remaining ADAT slots. File mode remains an unpaced
evidence capture and intentionally bypasses clock adaptation.
EOF
}

while (( "$#" )); do
  case "$1" in
    --fixture) MODE="fixture"; shift ;;
    --capture-device) CAPTURE_DEVICE="$2"; shift 2 ;;
    --output-file) OUTPUT_FILE="$2"; shift 2 ;;
    --output-device) OUTPUT_DEVICE="$2"; shift 2 ;;
    --output-profile) OUTPUT_PROFILE="$2"; shift 2 ;;
    --decoder-mode) DECODER_MODE="$2"; shift 2 ;;
    --require-surround-input) REQUIRE_SURROUND_INPUT=1; shift ;;
    --umc-card) UMC_CARD="$2"; shift 2 ;;
    --seconds) MAX_SECONDS="$2"; shift 2 ;;
    --work-dir) WORK_DIR="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ "$MODE" != "fixture" ]]; then
  [[ -n "$CAPTURE_DEVICE" ]] || { echo "--capture-device is required outside --fixture mode" >&2; exit 2; }
  if [[ -n "$OUTPUT_FILE" && -n "$OUTPUT_DEVICE" ]]; then
    echo "choose exactly one of --output-file or --output-device" >&2
    exit 2
  fi
  [[ -n "$OUTPUT_FILE" || -n "$OUTPUT_DEVICE" ]] || {
    echo "one of --output-file or --output-device is required" >&2
    exit 2
  }
  case "$DECODER_MODE" in
    joc-native|channel-bed|auto-bed) ;;
    *) echo "--decoder-mode must be joc-native, channel-bed or auto-bed" >&2; exit 2 ;;
  esac
  case "$OUTPUT_PROFILE" in
    native|umc1820-ada8200) ;;
    *) echo "--output-profile must be native or umc1820-ada8200" >&2; exit 2 ;;
  esac
  if [[ -n "$OUTPUT_FILE" && "$OUTPUT_PROFILE" != "native" ]]; then
    echo "--output-profile applies only to --output-device; raw evidence files stay canonical 12-channel Aurora PCM" >&2
    exit 2
  fi
fi

for cmd in python3 ffmpeg; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
[[ -f "$MANIFEST" && -f "$HARNESS_SOURCE" && -f "$PIPE_ADAPTER" && -f "$BED_SOURCE" && -f "$AUTO_BED_SOURCE" && -f "$ADAPTIVE_SOURCE" && -f "$UMC_PREFLIGHT" ]] || {
  echo "Aurora live eARC sources are incomplete" >&2
  exit 2
}

case "$BUILD_MODE" in
  debug) PROFILE_ARGS=(); PROFILE_DIR=debug ;;
  release) PROFILE_ARGS=(--release); PROFILE_DIR=release ;;
  *) echo "AURORA_JOC_BUILD_MODE must be debug or release" >&2; exit 2 ;;
esac

OWN_WORKDIR=0
if [[ -z "$WORK_DIR" ]]; then
  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/aurora-live-earc.XXXXXX")"
  OWN_WORKDIR=1
else
  mkdir -p "$WORK_DIR"
fi

cleanup() {
  if [[ "$KEEP_WORKDIR" == "1" ]]; then
    echo "Keeping live eARC workdir: $WORK_DIR" >&2
  elif (( OWN_WORKDIR == 1 )); then
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

# Avoid building/loading the native JOC codec/renderer when playing
# ordinary AC-3/DD+ beds. File capture needs no Rust build at all.
HARNESS_BIN=""
BRIDGE_LIB=""
ADAPTIVE_BIN=""
if [[ "$MODE" == "fixture" || "$DECODER_MODE" == "joc-native" ]]; then
  for cmd in git cargo rustup; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "missing native JOC build tool: $cmd" >&2; exit 2; }
  done
eval "$(python3 - "$MANIFEST" <<'PY'
import json, shlex, sys
manifest = json.load(open(sys.argv[1], encoding="utf-8"))
components = {c["id"]: c for c in manifest["components"]}
for prefix, cid in (("OMNIP", "omniphony"), ("HARLETTY", "harletty-bridge")):
    c = components[cid]
    for key, value in (
        ("UPSTREAM", c["upstream"]),
        ("VERSION", c["tested_version"]),
        ("COMMIT", c["pinned_commit"]),
    ):
        print(f"{prefix}_{key}={shlex.quote(value)}")
PY
)"

clone_pinned() {
  local url="$1" version="$2" commit="$3" dest="$4"
  if [[ ! -d "$dest/.git" ]]; then
    git clone --quiet --depth 1 --branch "$version" "$url" "$dest"
  fi
  local actual
  actual="$(git -C "$dest" rev-parse HEAD)"
  if [[ "$actual" != "$commit" ]]; then
    echo "pinned commit mismatch for $url: expected $commit got $actual" >&2
    exit 1
  fi
}

OMNIP_DIR="$WORK_DIR/Omniphony"
HARLETTY_DIR="$WORK_DIR/harletty-bridge"
HARLETTY_TARGET="$WORK_DIR/harletty-target"
HARNESS_DIR="$WORK_DIR/live-earc-harness"
HARNESS_TARGET="$WORK_DIR/live-earc-target"
ARTIFACTS="$WORK_DIR/artifacts"
mkdir -p "$HARLETTY_TARGET" "$HARNESS_DIR/src" "$HARNESS_TARGET" "$ARTIFACTS"

clone_pinned "$OMNIP_UPSTREAM" "$OMNIP_VERSION" "$OMNIP_COMMIT" "$OMNIP_DIR"
clone_pinned "$HARLETTY_UPSTREAM" "$HARLETTY_VERSION" "$HARLETTY_COMMIT" "$HARLETTY_DIR"

rustup toolchain install "$TOOLCHAIN" --profile minimal >/dev/null

CARGO_TARGET_DIR="$HARLETTY_TARGET" cargo +"$TOOLCHAIN" build --locked "${PROFILE_ARGS[@]}"   --manifest-path "$HARLETTY_DIR/Cargo.toml"   -p harletty-bridge

cat > "$HARNESS_DIR/Cargo.toml" <<EOF_CARGO
[package]
name = "aurora-live-earc-harness"
version = "0.1.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
abi_stable = "0.11"
bridge_api = { path = "$OMNIP_DIR/omniphony-renderer/bridge_api" }
spdif = { path = "$OMNIP_DIR/omniphony-renderer/spdif" }
aurora-core = { path = "$ROOT_DIR/crates/aurora-core" }
aurora-decoder-api = { path = "$ROOT_DIR/crates/aurora-decoder-api" }
aurora-renderer-vbap = { path = "$ROOT_DIR/crates/aurora-renderer-vbap" }
aurora-source-runtime = { path = "$ROOT_DIR/crates/aurora-source-runtime" }
aurora-realtime-engine = { path = "$ROOT_DIR/crates/aurora-realtime-engine" }
EOF_CARGO
cp "$HARNESS_SOURCE" "$HARNESS_DIR/src/main.rs"
mkdir -p "$HARNESS_DIR/src/bin"
cp "$ADAPTIVE_SOURCE" "$HARNESS_DIR/src/bin/aurora-adaptive-output.rs"

CARGO_TARGET_DIR="$HARNESS_TARGET" cargo +"$TOOLCHAIN" build "${PROFILE_ARGS[@]}"   --manifest-path "$HARNESS_DIR/Cargo.toml"

BRIDGE_LIB="$HARLETTY_TARGET/$PROFILE_DIR/libharletty_bridge.so"
HARNESS_BIN="$HARNESS_TARGET/$PROFILE_DIR/aurora-live-earc-harness"
ADAPTIVE_BIN="$HARNESS_TARGET/$PROFILE_DIR/aurora-adaptive-output"
[[ -f "$BRIDGE_LIB" && -x "$HARNESS_BIN" && -x "$ADAPTIVE_BIN" ]] || {
  echo "live eARC build products missing" >&2
  exit 1
}


elif [[ -n "$OUTPUT_DEVICE" ]]; then
  for cmd in cargo rustup; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "missing adaptive output build tool: $cmd" >&2; exit 2; }
  done
  # A minimal standalone process with ONLY the realtime-engine dependency:
  # no Harletty, Omniphony, PCM renderer or second decoder in channel-bed mode.
  ADAPTIVE_DIR="$WORK_DIR/adaptive-only"
  mkdir -p "$ADAPTIVE_DIR/src"
  cat > "$ADAPTIVE_DIR/Cargo.toml" <<EOF_ADAPTIVE
[package]
name = "aurora-adaptive-output"
version = "0.1.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
aurora-realtime-engine = { path = "$ROOT_DIR/crates/aurora-realtime-engine" }
EOF_ADAPTIVE
  cp "$ADAPTIVE_SOURCE" "$ADAPTIVE_DIR/src/main.rs"
  rustup toolchain install "$TOOLCHAIN" --profile minimal >/dev/null
  CARGO_TARGET_DIR="$WORK_DIR/adaptive-target" cargo +"$TOOLCHAIN" build "${PROFILE_ARGS[@]}" --manifest-path "$ADAPTIVE_DIR/Cargo.toml"
  ADAPTIVE_BIN="$WORK_DIR/adaptive-target/$PROFILE_DIR/aurora-adaptive-output"
  [[ -x "$ADAPTIVE_BIN" ]] || { echo "standalone adaptive output build missing" >&2; exit 1; }
fi

verify_s32_output() {
  python3 - "$1" <<'PY'
import array, math, pathlib, sys
path = pathlib.Path(sys.argv[1])
data = path.read_bytes()
frame_bytes = 12 * 4
if len(data) < frame_bytes * 100:
    raise SystemExit(f"12-channel output too short: {len(data)} bytes")
if len(data) % frame_bytes:
    raise SystemExit(f"output is not whole 12-channel S32_LE frames: {len(data)} bytes")
samples = array.array("i")
samples.frombytes(data)
if sys.byteorder != "little":
    samples.byteswap()
peak = max((abs(v) for v in samples), default=0)
if peak == 0:
    raise SystemExit("12-channel output is silent")
frames = len(samples) // 12
activity = []
for channel in range(12):
    vals = samples[channel::12]
    activity.append(max((abs(v) for v in vals), default=0))
# This public JOC fixture proves native objects and 12-channel rendering, but it is
# not an authored height-activity fixture. Preserve per-channel peaks as evidence
# without requiring a particular programme channel to be active.
print(
    "AURORA-LIVE-EARC-S32-PASS "
    f"frames={frames} bytes={len(data)} peak={peak} "
    "channel_peaks=" + ",".join(str(v) for v in activity)
)
PY
}

if [[ "$MODE" == "fixture" ]]; then
  JOC_FIXTURE="$HARLETTY_DIR/harletty/tests/fixtures/joc_atmos_1s.eac3"
  JOC_IEC="$ARTIFACTS/joc.spdif"
  OUT="$ARTIFACTS/joc-7.1.4.s32"
  PLAIN_IEC="$ARTIFACTS/plain.spdif"

  ffmpeg -nostdin -hide_banner -loglevel error -y     -i "$JOC_FIXTURE" -map 0:a:0 -c:a copy -f spdif "$JOC_IEC"

  cat "$JOC_IEC" | "$HARNESS_BIN" "$BRIDGE_LIB" > "$OUT"
  verify_s32_output "$OUT"
  "$ADAPTIVE_BIN" --self-test --channels 12 --sample-rate 48000 --output-profile native
  "$ADAPTIVE_BIN" --self-test --channels 12 --sample-rate 48000 --output-profile umc1820-ada8200
  python3 "$UMC_PREFLIGHT" --self-test

  ffmpeg -nostdin -hide_banner -loglevel error -y     -f lavfi -i "anullsrc=channel_layout=5.1:sample_rate=48000"     -t 0.25 -c:a eac3 -b:a 448k -f spdif "$PLAIN_IEC"

  set +e
  cat "$PLAIN_IEC" | "$HARNESS_BIN" "$BRIDGE_LIB" > "$ARTIFACTS/plain-output.s32" 2>"$ARTIFACTS/plain-error.log"
  PLAIN_RC=$?
  set -e
  if [[ "$PLAIN_RC" == "0" ]]; then
    echo "plain E-AC-3 incorrectly passed the native-JOC gate" >&2
    exit 1
  fi
  echo "AURORA-LIVE-EARC-PLAIN-FAIL-CLOSED-PASS rc=$PLAIN_RC"
  echo "AURORA LIVE EARC VBAP FIXTURE PASS"
  exit 0
fi

STATUS="$ARTIFACTS/capture-status.json"
HW_PARAMS="$ARTIFACTS/arecord-hw-params.log"
CAPTURE_STDERR="$ARTIFACTS/arecord-stderr.log"
IEC_CAPTURE="$ARTIFACTS/live-earc.spdif"
INGRESS_REPORT="$ARTIFACTS/live-ingress-report.json"

PIPE_ARGS=(
  --device "$CAPTURE_DEVICE"
  --status "$STATUS"
  --hw-params-log "$HW_PARAMS"
  --stderr-log "$CAPTURE_STDERR"
)
if [[ -n "$MAX_SECONDS" ]]; then
  PIPE_ARGS+=(--max-seconds "$MAX_SECONDS")
fi

if [[ "$DECODER_MODE" == "auto-bed" ]]; then
  DECODER=(python3 "$AUTO_BED_SOURCE")
elif [[ "$DECODER_MODE" == "channel-bed" ]]; then
  DECODER=(python3 "$BED_SOURCE")
  if (( REQUIRE_SURROUND_INPUT == 1 )); then
    DECODER+=(--require-surround-input)
  fi
else
  DECODER=("$HARNESS_BIN" "$BRIDGE_LIB")
fi

echo "AURORA LIVE EARC selected_decoder=$DECODER_MODE output=7.1.4 objects_decoded=$([[ "$DECODER_MODE" == "joc-native" ]] && echo true || echo false)" >&2

if [[ -n "$OUTPUT_FILE" ]]; then
  mkdir -p "$(dirname "$OUTPUT_FILE")"
  python3 "$PIPE_ADAPTER" "${PIPE_ARGS[@]}"     | tee "$IEC_CAPTURE"     | "${DECODER[@]}"     > "$OUTPUT_FILE"
  verify_s32_output "$OUTPUT_FILE"
else
  command -v aplay >/dev/null 2>&1 || {
    echo "aplay is required for --output-device" >&2
    exit 2
  }
  APLAY_CHANNELS=12
  if [[ "$OUTPUT_PROFILE" == "umc1820-ada8200" ]]; then
    APLAY_CHANNELS=20
    python3 "$UMC_PREFLIGHT" --card "$UMC_CARD" --report "$ARTIFACTS/umc1820-preflight.json"
  fi
  python3 "$PIPE_ADAPTER" "${PIPE_ARGS[@]}"     | tee "$IEC_CAPTURE"     | "${DECODER[@]}"     | "$ADAPTIVE_BIN" --channels 12 --sample-rate 48000 --output-profile "$OUTPUT_PROFILE"     | aplay -D "$OUTPUT_DEVICE" -t raw -f S32_LE -c "$APLAY_CHANNELS" -r 48000
fi

INGRESS_POLICY=(--max-invalid-bursts 0 --max-unclassified-bytes 0 --minimum-valid-bursts 2 --require-locked-end)
if [[ "$DECODER_MODE" != "auto-bed" ]]; then
  INGRESS_POLICY+=(--max-relocks 0)
fi
python3 "$ROOT_DIR/validation/physical/aurora_live_ingress.py" analyze --input "$IEC_CAPTURE" --report "$INGRESS_REPORT" "${INGRESS_POLICY[@]}"

echo "AURORA LIVE EARC PHYSICAL RUN COMPLETE decoder_mode=$DECODER_MODE (hardware evidence still required)"
echo "evidence: $ARTIFACTS"
