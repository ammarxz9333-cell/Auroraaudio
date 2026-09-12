#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EVAL_CONFIG="$ROOT_DIR/config/omniphony-evaluation-v1.json"
BASELINE_TEST="$ROOT_DIR/validation/immersive/test-joc-stack.sh"
ANALYZER="$ROOT_DIR/validation/immersive/omniphony_candidate_evidence.py"
OUTPUT_DIR=${1:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-omniphony-eval.XXXXXX")"}
TOOLCHAIN="${AURORA_EXTERNAL_RUST_TOOLCHAIN:-stable}"

for cmd in git python3 ffmpeg rustup cargo timeout; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
for path in "$EVAL_CONFIG" "$BASELINE_TEST" "$ANALYZER"; do
  [[ -f "$path" ]] || { echo "missing evaluation dependency: $path" >&2; exit 2; }
done
[[ "$(uname -s)" == "Linux" ]] || { echo "Omniphony evaluation lane currently targets Linux." >&2; exit 2; }

mkdir -p "$OUTPUT_DIR"
STABLE_DIR="$OUTPUT_DIR/stable"
STABLE_HARLETTY_TARGET="$OUTPUT_DIR/stable-harletty-target"
STABLE_OMNIP_TARGET="$OUTPUT_DIR/stable-omniphony-target"
STABLE_HARNESS_TARGET="$OUTPUT_DIR/stable-harness-target"
CANDIDATE_DIR="$OUTPUT_DIR/candidate"
CANDIDATE_TARGET="$OUTPUT_DIR/candidate-target"
HARNESS_DIR="$OUTPUT_DIR/candidate-bridge-harness"
HARNESS_TARGET="$OUTPUT_DIR/candidate-bridge-harness-target"
BRIDGE_LOG="$OUTPUT_DIR/candidate-bridge.log"
CANDIDATE_PCM="$OUTPUT_DIR/candidate-7.1.4.f32"
CANDIDATE_LOG="$OUTPUT_DIR/candidate-orender.log"
REPORT="$OUTPUT_DIR/omniphony-candidate-evidence.json"

readarray -t PINS < <(python3 - "$EVAL_CONFIG" <<'PY'
import json, sys
p = json.load(open(sys.argv[1], encoding="utf-8"))
for section in ("stable", "candidate"):
    item = p[section]
    print(item["upstream"])
    print(item["commit"])
PY
)
STABLE_UPSTREAM=${PINS[0]}
STABLE_COMMIT=${PINS[1]}
CANDIDATE_UPSTREAM=${PINS[2]}
CANDIDATE_COMMIT=${PINS[3]}

printf '\n== Omniphony evaluation: reproduce stable v0.5.2 lane ==\n'
AURORA_JOC_TEST_WORKDIR="$STABLE_DIR" \
AURORA_KEEP_JOC_TEST_WORKDIR=1 \
AURORA_JOC_BUILD_MODE=release \
AURORA_EXTERNAL_RUST_TOOLCHAIN="$TOOLCHAIN" \
AURORA_HARLETTY_TARGET_DIR="$STABLE_HARLETTY_TARGET" \
AURORA_OMNIP_TARGET_DIR="$STABLE_OMNIP_TARGET" \
AURORA_JOC_HARNESS_TARGET_DIR="$STABLE_HARNESS_TARGET" \
bash "$BASELINE_TEST"

STABLE_OMNIP="$STABLE_DIR/Omniphony"
HARLETTY_DIR="$STABLE_DIR/harletty-bridge"
STABLE_PCM="$STABLE_DIR/joc_atmos_7_1_4.f32"
STABLE_LOG="$STABLE_DIR/orender-joc.log"
IEC_FILE="$STABLE_DIR/joc_atmos_1s.spdif"
BRIDGE_LIB="$STABLE_HARLETTY_TARGET/release/libharletty_bridge.so"
STABLE_LABELS="$STABLE_OMNIP/omniphony-renderer/bridge_api/src/labels.rs"

for path in "$STABLE_PCM" "$STABLE_LOG" "$IEC_FILE" "$BRIDGE_LIB" "$STABLE_LABELS"; do
  [[ -f "$path" ]] || { echo "stable lane artifact missing: $path" >&2; exit 1; }
done
ACTUAL_STABLE="$(git -C "$STABLE_OMNIP" rev-parse HEAD)"
[[ "$ACTUAL_STABLE" == "$STABLE_COMMIT" ]] || {
  echo "stable Omniphony pin moved: expected $STABLE_COMMIT got $ACTUAL_STABLE" >&2
  exit 1
}

printf '\n== Omniphony evaluation: clone exact candidate commit ==\n'
rm -rf "$CANDIDATE_DIR"
git clone --quiet --filter=blob:none --no-checkout "$CANDIDATE_UPSTREAM" "$CANDIDATE_DIR"
git -C "$CANDIDATE_DIR" fetch --quiet --depth 1 origin "$CANDIDATE_COMMIT"
git -C "$CANDIDATE_DIR" checkout --quiet --detach "$CANDIDATE_COMMIT"
ACTUAL_CANDIDATE="$(git -C "$CANDIDATE_DIR" rev-parse HEAD)"
[[ "$ACTUAL_CANDIDATE" == "$CANDIDATE_COMMIT" ]] || {
  echo "candidate commit mismatch: expected $CANDIDATE_COMMIT got $ACTUAL_CANDIDATE" >&2
  exit 1
}

CANDIDATE_RENDERER="$CANDIDATE_DIR/omniphony-renderer"
CANDIDATE_LABELS="$CANDIDATE_RENDERER/bridge_api/src/labels.rs"
CANDIDATE_LAYOUT="$CANDIDATE_DIR/layouts/7.1.4.yaml"
for path in "$CANDIDATE_LABELS" "$CANDIDATE_LAYOUT" "$CANDIDATE_RENDERER/Cargo.toml"; do
  [[ -f "$path" ]] || { echo "candidate repository shape incompatible: missing $path" >&2; exit 1; }
done

printf '\n== Omniphony evaluation: build exact candidate externally ==\n'
rustup toolchain install "$TOOLCHAIN" --profile minimal >/dev/null
mkdir -p "$CANDIDATE_TARGET"
CARGO_TARGET_DIR="$CANDIDATE_TARGET" cargo +"$TOOLCHAIN" build --locked --release \
  --manifest-path "$CANDIDATE_RENDERER/Cargo.toml" \
  -p omniphony-renderer
CANDIDATE_ORENDER="$CANDIDATE_TARGET/release/orender"
[[ -x "$CANDIDATE_ORENDER" ]] || { echo "candidate orender binary missing: $CANDIDATE_ORENDER" >&2; exit 1; }

printf '\n== Omniphony evaluation: compile candidate bridge-ABI/object harness ==\n'
mkdir -p "$HARNESS_DIR/src" "$HARNESS_TARGET"
cat > "$HARNESS_DIR/Cargo.toml" <<EOF_CARGO
[package]
name = "aurora-omniphony-candidate-bridge"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
abi_stable = "0.11"
bridge_api = { path = "$CANDIDATE_RENDERER/bridge_api" }
spdif = { path = "$CANDIDATE_RENDERER/spdif" }
EOF_CARGO
cat > "$HARNESS_DIR/src/main.rs" <<'EOF_RS'
use abi_stable::library::RootModule;
use bridge_api::{BridgeLibRef, RInputTransport};
use spdif::SpdifParser;
use std::{env, fs, path::Path};

fn main() {
    let mut args = env::args().skip(1);
    let bridge_path = args.next().expect("bridge path");
    let carrier_path = args.next().expect("carrier path");
    assert!(args.next().is_none(), "unexpected extra args");

    let lib = BridgeLibRef::load_from_file(Path::new(&bridge_path)).expect("candidate bridge API must load pinned Harletty bridge ABI");
    let mut bridge = (lib.new_bridge())(false);
    let carrier = fs::read(carrier_path).expect("read IEC61937 JOC fixture");
    let mut parser = SpdifParser::new();
    let mut packets = 0usize;
    let mut frames = 0usize;
    let mut metadata_frames = 0usize;
    let mut events = 0usize;
    let mut object_channels = 0usize;
    let mut saw_objects = false;

    for chunk in carrier.chunks(997) {
        parser.push_bytes(chunk);
        while let Some(packet) = parser.get_next_packet() {
            packets += 1;
            assert_eq!(packet.data_type, 0x15, "fixture changed away from E-AC-3");
            let result = bridge.push_packet(
                packet.payload.as_slice().into(),
                RInputTransport::Iec61937,
                packet.data_type,
            );
            assert!(result.error_message.is_empty(), "bridge error: {}", result.error_message.as_str());
            frames += result.frames.len();
            for frame in result.frames.iter() {
                if !frame.metadata.is_empty() { metadata_frames += 1; }
                for meta in frame.metadata.iter() {
                    events += meta.events.len();
                    object_channels += meta.object_channels.len();
                }
            }
            saw_objects |= bridge.has_objects();
        }
    }
    assert!(packets > 0 && frames > 0, "candidate bridge API yielded no decoded frames");
    assert!(metadata_frames > 0 && events > 0 && object_channels > 0 && saw_objects, "candidate bridge API lost JOC object semantics");
    println!(
        "CANDIDATE-BRIDGE-PASS packets={packets} frames={frames} metadata_frames={metadata_frames} events={events} object_channels={object_channels} saw_objects={saw_objects}"
    );
}
EOF_RS
CARGO_TARGET_DIR="$HARNESS_TARGET" cargo +"$TOOLCHAIN" run --quiet --release \
  --manifest-path "$HARNESS_DIR/Cargo.toml" -- \
  "$BRIDGE_LIB" "$IEC_FILE" | tee "$BRIDGE_LOG"

printf '\n== Omniphony evaluation: render same JOC fixture through candidate ==\n'
if ! timeout 45s env RUST_LOG="${RUST_LOG:-info}" "$CANDIDATE_ORENDER" "$IEC_FILE" \
  --bridge-path "$BRIDGE_LIB" \
  --enable-vbap \
  --speaker-layout "$CANDIDATE_LAYOUT" \
  --output-backend file \
  --output-file "$CANDIDATE_PCM" \
  --output-file-format raw-f32 \
  >"$CANDIDATE_LOG" 2>&1; then
  echo "candidate orender failed or did not finish finite fixture:" >&2
  cat "$CANDIDATE_LOG" >&2
  exit 1
fi
[[ -s "$CANDIDATE_PCM" ]] || { echo "candidate produced no PCM output" >&2; exit 1; }

printf '\n== Omniphony evaluation: compare evidence ==\n'
python3 "$ANALYZER" analyze \
  --stable-pcm "$STABLE_PCM" \
  --candidate-pcm "$CANDIDATE_PCM" \
  --stable-labels "$STABLE_LABELS" \
  --candidate-labels "$CANDIDATE_LABELS" \
  --bridge-log "$BRIDGE_LOG" \
  --stable-log "$STABLE_LOG" \
  --candidate-log "$CANDIDATE_LOG" \
  --stable-commit "$STABLE_COMMIT" \
  --candidate-commit "$CANDIDATE_COMMIT" \
  --output "$REPORT"

echo "AURORA OMNIPHONY EVALUATION PASS"
echo "report=$REPORT"
