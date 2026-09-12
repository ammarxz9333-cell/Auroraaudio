#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REFERENCE_TEST="$ROOT_DIR/validation/immersive/test-dolby-official-joc-temporal.sh"
BASELINE_TEST="$ROOT_DIR/validation/immersive/test-joc-stack.sh"
ANALYZER="$ROOT_DIR/validation/immersive/aurora_joc_moving_evidence.py"
OUTPUT_DIR=${1:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-moving-joc.XXXXXX")"}
EXPECTED_DERIVED_SHA256="0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0"
BUILD_MODE="${AURORA_JOC_BUILD_MODE:-release}"
TOOLCHAIN="${AURORA_EXTERNAL_RUST_TOOLCHAIN:-stable}"
TIMEOUT_SECONDS="${AURORA_MOVING_JOC_TIMEOUT_SECONDS:-130}"

case "$BUILD_MODE" in
  debug) PROFILE_ARGS=(); PROFILE_DIR="debug" ;;
  release) PROFILE_ARGS=(--release); PROFILE_DIR="release" ;;
  *) echo "unsupported AURORA_JOC_BUILD_MODE: $BUILD_MODE" >&2; exit 2 ;;
esac
case "$TIMEOUT_SECONDS" in
  ''|*[!0-9]*) echo "AURORA_MOVING_JOC_TIMEOUT_SECONDS must be a positive integer" >&2; exit 2 ;;
esac
(( TIMEOUT_SECONDS >= 1 )) || { echo "AURORA_MOVING_JOC_TIMEOUT_SECONDS must be at least 1" >&2; exit 2; }

for cmd in python3 ffmpeg cargo timeout sha256sum; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
command -v "${OPENJOC_BIN:-openjoc}" >/dev/null 2>&1 || {
  echo "OPENJOC_BIN must point to the pinned OpenJOC reference binary" >&2
  exit 2
}
for path in "$REFERENCE_TEST" "$BASELINE_TEST" "$ANALYZER"; do
  [[ -f "$path" ]] || { echo "missing validation dependency: $path" >&2; exit 2; }
done

mkdir -p "$OUTPUT_DIR"
REFERENCE_DIR="$OUTPUT_DIR/reference"
STACK_DIR="$OUTPUT_DIR/stack"
MOVING_DIR="$OUTPUT_DIR/aurora-moving"
mkdir -p "$MOVING_DIR"

printf '\n== Aurora moving JOC phase: reproduce the checksum-pinned reference carrier ==\n'
bash "$REFERENCE_TEST" "$REFERENCE_DIR"
DERIVED="$REFERENCE_DIR/Living-Room-Atmos_after_first_au.ec3"
[[ -s "$DERIVED" ]] || { echo "derived moving carrier missing: $DERIVED" >&2; exit 1; }
DERIVED_SHA256="$(sha256sum "$DERIVED" | awk '{print $1}')"
[[ "$DERIVED_SHA256" == "$EXPECTED_DERIVED_SHA256" ]] || {
  echo "derived moving carrier hash mismatch: $DERIVED_SHA256" >&2
  exit 1
}

printf '\n== Aurora moving JOC phase: build and smoke-test pinned Harletty/Omniphony stack ==\n'
AURORA_JOC_TEST_WORKDIR="$STACK_DIR" \
AURORA_KEEP_JOC_TEST_WORKDIR=1 \
AURORA_JOC_BUILD_MODE="$BUILD_MODE" \
AURORA_EXTERNAL_RUST_TOOLCHAIN="$TOOLCHAIN" \
bash "$BASELINE_TEST"

HARLETTY_DIR="$STACK_DIR/harletty-bridge"
OMNIP_DIR="$STACK_DIR/Omniphony"
HARLETTY_TARGET_DIR="${AURORA_HARLETTY_TARGET_DIR:-$HARLETTY_DIR/target}"
OMNIP_TARGET_DIR="${AURORA_OMNIP_TARGET_DIR:-$OMNIP_DIR/omniphony-renderer/target}"
BRIDGE_LIB="$HARLETTY_TARGET_DIR/$PROFILE_DIR/libharletty_bridge.so"
ORENDER="$OMNIP_TARGET_DIR/$PROFILE_DIR/orender"
LAYOUT="$OMNIP_DIR/layouts/7.1.4.yaml"
MOVING_IEC="$MOVING_DIR/dolby-moving.spdif"
TELEMETRY="$MOVING_DIR/aurora-bridge-telemetry.json"
UNPACED_RENDER="$MOVING_DIR/aurora-moving-7.1.4.f32"
UNPACED_LOG="$MOVING_DIR/orender-unpaced.log"
PACED_RENDER="$MOVING_DIR/aurora-moving-paced-7.1.4.f32"
PACED_LOG="$MOVING_DIR/orender-paced.log"
PACING_JSON="$MOVING_DIR/pacing.json"
REPORT="$MOVING_DIR/aurora-joc-moving-evidence.json"

for path in "$BRIDGE_LIB" "$ORENDER" "$LAYOUT"; do
  [[ -f "$path" ]] || { echo "missing built Aurora validation dependency: $path" >&2; exit 1; }
done

printf '\n== Aurora moving JOC phase: wrap exact moving suffix in IEC61937 ==\n'
ffmpeg -nostdin -hide_banner -loglevel error -y \
  -i "$DERIVED" -map 0:a:0 -c:a copy -f spdif "$MOVING_IEC"

python3 - "$MOVING_IEC" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
data = p.read_bytes()
sync = bytes.fromhex("72f81f4e")
if not data.startswith(sync):
    raise SystemExit("moving IEC61937 carrier does not start with sync")
second = data.find(sync, 4)
if second <= 0:
    raise SystemExit("moving IEC61937 carrier contains fewer than two bursts")
if len(data) % second:
    raise SystemExit(f"carrier length {len(data)} is not a multiple of burst size {second}")
bursts = len(data) // second
if bursts != 2360:
    raise SystemExit(f"expected 2360 IEC61937 bursts, got {bursts}")
for index in range(bursts):
    offset = index * second
    if data[offset:offset + 4] != sync:
        raise SystemExit(f"IEC61937 sync missing at burst {index}")
    if data[offset + 4] & 0x1F != 0x15:
        raise SystemExit(f"non-E-AC-3 IEC61937 data type at burst {index}")
print(f"AURORA-MOVING-IEC61937-PASS bursts={bursts} burst_bytes={second} bytes={len(data)}")
PY

HARNESS_DIR="$MOVING_DIR/telemetry-harness"
HARNESS_TARGET_DIR="${AURORA_MOVING_JOC_HARNESS_TARGET_DIR:-$MOVING_DIR/telemetry-target}"
mkdir -p "$HARNESS_DIR/src" "$HARNESS_TARGET_DIR"
cat > "$HARNESS_DIR/Cargo.toml" <<EOF_CARGO
[package]
name = "aurora-moving-joc-telemetry"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
abi_stable = "0.11"
bridge_api = { path = "$OMNIP_DIR/omniphony-renderer/bridge_api" }
spdif = { path = "$OMNIP_DIR/omniphony-renderer/spdif" }
serde_json = "1"
EOF_CARGO

cat > "$HARNESS_DIR/src/main.rs" <<'EOF_RS'
use abi_stable::library::RootModule;
use bridge_api::{BridgeLibRef, REvent, RInputTransport};
use serde_json::json;
use spdif::SpdifParser;
use std::{
    collections::BTreeMap,
    env,
    fs,
    path::Path,
};

#[derive(Default, Debug)]
struct ObjectStats {
    event_count: u64,
    position_event_count: u64,
    position_change_count: u64,
    first_sample: Option<u64>,
    last_sample: Option<u64>,
    first_change_sample: Option<u64>,
    last_change_sample: Option<u64>,
    position_min: Option<[f64; 3]>,
    position_max: Option<[f64; 3]>,
    last_position: Option<[f64; 3]>,
}

impl ObjectStats {
    fn observe(&mut self, event: &REvent) {
        self.event_count += 1;
        self.first_sample.get_or_insert(event.sample_pos);
        self.last_sample = Some(event.sample_pos);
        if !event.has_pos {
            return;
        }
        self.position_event_count += 1;
        let pos = event.pos;
        match self.position_min.as_mut() {
            Some(minimum) => {
                for axis in 0..3 {
                    minimum[axis] = minimum[axis].min(pos[axis]);
                }
            }
            None => self.position_min = Some(pos),
        }
        match self.position_max.as_mut() {
            Some(maximum) => {
                for axis in 0..3 {
                    maximum[axis] = maximum[axis].max(pos[axis]);
                }
            }
            None => self.position_max = Some(pos),
        }
        if let Some(previous) = self.last_position {
            let changed = (0..3).any(|axis| (previous[axis] - pos[axis]).abs() > 1.0e-9);
            if changed {
                self.position_change_count += 1;
                self.first_change_sample.get_or_insert(event.sample_pos);
                self.last_change_sample = Some(event.sample_pos);
            }
        }
        self.last_position = Some(pos);
    }
}

fn main() {
    let mut args = env::args().skip(1);
    let bridge_path = args.next().expect("bridge path argument");
    let carrier_path = args.next().expect("IEC61937 carrier argument");
    let output_path = args.next().expect("telemetry JSON output argument");
    assert!(args.next().is_none(), "unexpected extra arguments");

    let lib = BridgeLibRef::load_from_file(Path::new(&bridge_path)).expect("load Harletty bridge");
    let mut bridge = (lib.new_bridge())(false);
    let carrier = fs::read(&carrier_path).expect("read IEC61937 carrier");
    let mut parser = SpdifParser::new();
    let mut packets = 0u64;
    let mut frames = 0u64;
    let mut total_samples = 0u64;
    let mut sample_rate: Option<u32> = None;
    let mut metadata_frames = 0u64;
    let mut events = 0u64;
    let mut object_channel_declarations = 0u64;
    let mut reset_count = 0u64;
    let mut metadata_positions_monotonic = true;
    let mut previous_metadata_sample: Option<u64> = None;
    let mut objects: BTreeMap<u32, ObjectStats> = BTreeMap::new();

    for chunk in carrier.chunks(997) {
        parser.push_bytes(chunk);
        while let Some(packet) = parser.get_next_packet() {
            packets += 1;
            assert_eq!(packet.data_type, 0x15, "non-E-AC-3 IEC61937 burst");
            let result = bridge.push_packet(
                packet.payload.as_slice().into(),
                RInputTransport::Iec61937,
                packet.data_type,
            );
            assert!(
                result.error_message.is_empty(),
                "bridge error after packet {packets}: {}",
                result.error_message.as_str()
            );
            if result.did_reset {
                reset_count += 1;
            }
            for frame in result.frames.iter() {
                frames += 1;
                total_samples += u64::from(frame.sample_count);
                match sample_rate {
                    Some(rate) => assert_eq!(rate, frame.sampling_frequency, "sample rate changed mid-stream"),
                    None => sample_rate = Some(frame.sampling_frequency),
                }
                for meta in frame.metadata.iter() {
                    metadata_frames += 1;
                    if let Some(previous) = previous_metadata_sample {
                        if meta.sample_pos < previous {
                            metadata_positions_monotonic = false;
                        }
                    }
                    previous_metadata_sample = Some(meta.sample_pos);
                    object_channel_declarations += meta.object_channels.len() as u64;
                    events += meta.events.len() as u64;
                    for event in meta.events.iter() {
                        objects.entry(event.id).or_default().observe(event);
                    }
                }
            }
        }
    }

    assert!(packets > 0, "no IEC61937 packets parsed");
    assert!(frames > 0, "Harletty emitted no decoded frames");
    assert!(bridge.is_ready(), "bridge never became ready");
    assert!(bridge.has_objects(), "bridge does not report dynamic objects at end of moving carrier");
    assert!(metadata_frames > 0 && events > 0, "moving carrier emitted no object metadata");
    assert!(metadata_positions_monotonic, "metadata sample positions regressed");

    let object_values: Vec<_> = objects
        .iter()
        .map(|(id, stats)| {
            json!({
                "id": id,
                "event_count": stats.event_count,
                "position_event_count": stats.position_event_count,
                "position_change_count": stats.position_change_count,
                "first_sample": stats.first_sample,
                "last_sample": stats.last_sample,
                "first_change_sample": stats.first_change_sample,
                "last_change_sample": stats.last_change_sample,
                "position_min": stats.position_min,
                "position_max": stats.position_max,
            })
        })
        .collect();
    let varying_count = objects
        .values()
        .filter(|stats| stats.position_change_count > 0)
        .count();
    assert!(varying_count > 0, "Harletty metadata contained no changing object position");

    let payload = json!({
        "schema_version": 1,
        "source": "Aurora pinned Harletty bridge via IEC61937",
        "transport": {
            "data_type": 0x15,
            "packets": packets,
        },
        "decode": {
            "frames": frames,
            "sample_rate_hz": sample_rate.unwrap_or(0),
            "total_samples": total_samples,
            "reset_count": reset_count,
            "bridge_ready": bridge.is_ready(),
            "bridge_has_objects": bridge.has_objects(),
        },
        "metadata": {
            "metadata_frames": metadata_frames,
            "events": events,
            "object_channel_declarations": object_channel_declarations,
            "sample_positions_monotonic": metadata_positions_monotonic,
            "object_count": objects.len(),
            "position_varying_object_count": varying_count,
            "objects": object_values,
        },
    });
    fs::write(&output_path, serde_json::to_vec_pretty(&payload).expect("serialize telemetry"))
        .expect("write telemetry JSON");
    println!(
        "AURORA-MOVING-BRIDGE-PASS packets={packets} frames={frames} metadata_frames={metadata_frames} events={events} objects={} varying_objects={varying_count} total_samples={total_samples}",
        objects.len()
    );
}
EOF_RS

printf '\n== Aurora moving JOC phase: capture Harletty object trajectories ==\n'
CARGO_TARGET_DIR="$HARNESS_TARGET_DIR" cargo +"$TOOLCHAIN" run --quiet "${PROFILE_ARGS[@]}" \
  --manifest-path "$HARNESS_DIR/Cargo.toml" -- \
  "$BRIDGE_LIB" "$MOVING_IEC" "$TELEMETRY"
[[ -s "$TELEMETRY" ]] || { echo "Aurora bridge telemetry was not created" >&2; exit 1; }

printf '\n== Aurora moving JOC phase: render full moving carrier to 7.1.4 ==\n'
if ! RUST_LOG="${RUST_LOG:-info}" "$ORENDER" "$MOVING_IEC" \
  --bridge-path "$BRIDGE_LIB" \
  --enable-vbap \
  --speaker-layout "$LAYOUT" \
  --output-backend file \
  --output-file "$UNPACED_RENDER" \
  --output-file-format raw-f32 \
  >"$UNPACED_LOG" 2>&1; then
  echo "Omniphony failed on moving JOC carrier:" >&2
  cat "$UNPACED_LOG" >&2
  exit 1
fi

python3 - "$MOVING_IEC" "$UNPACED_RENDER" <<'PY'
import pathlib, sys
carrier = pathlib.Path(sys.argv[1]).read_bytes()
render = pathlib.Path(sys.argv[2]).read_bytes()
sync = bytes.fromhex("72f81f4e")
second = carrier.find(sync, 4)
if second <= 0 or len(carrier) % second:
    raise SystemExit("cannot derive IEC61937 burst geometry")
bursts = len(carrier) // second
frame_bytes = 12 * 4
if len(render) % frame_bytes:
    raise SystemExit("unpaced Omniphony output is not whole 12-channel f32 frames")
frames = len(render) // frame_bytes
if frames == 0 or frames % bursts:
    raise SystemExit(f"cannot derive media cadence: frames={frames} bursts={bursts}")
frames_per_burst = frames // bursts
if frames_per_burst != 1536:
    raise SystemExit(f"expected 1536 rendered frames per E-AC-3 burst, got {frames_per_burst}")
print(f"AURORA-MOVING-7.1.4-SHAPE-PASS bursts={bursts} frames={frames} frames_per_burst={frames_per_burst} media_seconds={frames / 48000:.6f}")
PY

printf '\n== Aurora moving JOC phase: media-paced full-carrier render ==\n'
START_NS="$(python3 - <<'PY'
import time
print(time.monotonic_ns())
PY
)"
set +e
python3 - "$MOVING_IEC" "$UNPACED_RENDER" <<'PY' | \
  RUST_LOG="${RUST_LOG:-info}" timeout --signal=TERM --kill-after=5s "${TIMEOUT_SECONDS}s" \
  "$ORENDER" - \
    --bridge-path "$BRIDGE_LIB" \
    --enable-vbap \
    --speaker-layout "$LAYOUT" \
    --output-backend file \
    --output-file "$PACED_RENDER" \
    --output-file-format raw-f32 \
    >"$PACED_LOG" 2>&1
import pathlib, sys, time
carrier = pathlib.Path(sys.argv[1]).read_bytes()
render = pathlib.Path(sys.argv[2]).read_bytes()
sync = bytes.fromhex("72f81f4e")
second = carrier.find(sync, 4)
if second <= 0 or len(carrier) % second:
    raise SystemExit("cannot derive fixed IEC61937 burst size")
bursts = len(carrier) // second
frame_bytes = 12 * 4
if len(render) % frame_bytes:
    raise SystemExit("unpaced render shape invalid")
frames = len(render) // frame_bytes
if frames % bursts:
    raise SystemExit("render cadence is not integral per IEC61937 burst")
frames_per_burst = frames // bursts
interval = frames_per_burst / 48000.0
out = sys.stdout.buffer
deadline = time.monotonic()
for burst_index in range(bursts):
    offset = burst_index * second
    burst = carrier[offset:offset + second]
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

python3 - "$MOVING_IEC" "$UNPACED_RENDER" "$PACED_RENDER" "$PACED_LOG" "$PACING_JSON" "$START_NS" "$END_NS" "$FEED_RC" "$RENDER_RC" <<'PY'
import json, pathlib, re, sys
carrier_path, unpaced_path, paced_path, log_path, output_path = map(pathlib.Path, sys.argv[1:6])
start_ns, end_ns, feeder_rc, renderer_rc = map(int, sys.argv[6:10])
carrier = carrier_path.read_bytes()
unpaced = unpaced_path.read_bytes()
paced = paced_path.read_bytes() if paced_path.exists() else b""
sync = bytes.fromhex("72f81f4e")
second = carrier.find(sync, 4)
if second <= 0 or len(carrier) % second:
    raise SystemExit("cannot derive burst geometry for pacing report")
bursts = len(carrier) // second
frame_bytes = 12 * 4
if len(unpaced) % frame_bytes or len(paced) % frame_bytes:
    raise SystemExit("paced/unpaced output is not whole 12-channel frames")
expected_frames = len(unpaced) // frame_bytes
actual_frames = len(paced) // frame_bytes
media_seconds = expected_frames / 48000.0
elapsed = (end_ns - start_ns) / 1_000_000_000.0
realtime_factor = media_seconds / elapsed if elapsed > 0 else 0.0
log = log_path.read_text(encoding="utf-8", errors="replace") if log_path.exists() else ""
xrun_markers = re.findall(r"(?i)\b(?:xrun|underrun|overrun)\b", log)
minimum_elapsed = media_seconds * 0.85
maximum_elapsed = media_seconds * 1.35 + 1.5
passed = (
    feeder_rc == 0
    and renderer_rc == 0
    and expected_frames > 0
    and actual_frames == expected_frames
    and not xrun_markers
    and elapsed >= minimum_elapsed
    and elapsed <= maximum_elapsed
)
payload = {
    "schema_version": 1,
    "status": "pass" if passed else "fail",
    "bursts": bursts,
    "expected_frames": expected_frames,
    "actual_frames": actual_frames,
    "media_seconds": media_seconds,
    "elapsed_seconds": elapsed,
    "realtime_factor": realtime_factor,
    "minimum_elapsed_seconds": minimum_elapsed,
    "maximum_elapsed_seconds": maximum_elapsed,
    "xrun_marker_count": len(xrun_markers),
    "feeder_exit_code": feeder_rc,
    "renderer_exit_code": renderer_rc,
}
output_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(
    "AURORA-MOVING-PACED-" + ("PASS" if passed else "FAIL") + " "
    f"bursts={bursts} frames={actual_frames} media_seconds={media_seconds:.3f} "
    f"elapsed_seconds={elapsed:.3f} realtime_factor={realtime_factor:.3f} xruns={len(xrun_markers)}"
)
if not passed:
    raise SystemExit(1)
PY

printf '\n== Aurora moving JOC phase: fail-closed Aurora-side evidence analysis ==\n'
python3 "$ANALYZER" analyze \
  --input "$DERIVED" \
  --expected-sha256 "$EXPECTED_DERIVED_SHA256" \
  --provenance "byte-identical no-reencode suffix established by PR #140; official Dolby Online Delivery Kit v1.4.1 source" \
  --telemetry "$TELEMETRY" \
  --pcm "$UNPACED_RENDER" \
  --pacing "$PACING_JSON" \
  --sample-rate 48000 \
  --channels 12 \
  --output "$REPORT"

python3 - "$REPORT" <<'PY'
import json, pathlib, sys
report = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
if report.get("verdict") != "pass":
    raise SystemExit(f"Aurora moving-JOC evidence did not pass: {report.get('primary_reason')!r}")
bridge = report["aurora_bridge"]
meta = report["aurora_object_metadata"]
render = report["aurora_rendered_7_1_4"]
pacing = report["pacing_health"]
print(
    "AURORA-MOVING-JOC-PROOF-PASS "
    f"packets={bridge['packets']} frames={bridge['frames']} events={bridge['events']} "
    f"varying_objects={len(meta['position_varying_object_ids'])} "
    f"render_windows={render['window_count']} render_profile_l1={render['max_normalized_profile_l1_from_first']:.6f} "
    f"realtime_factor={pacing['realtime_factor']:.3f}"
)
PY

echo "AURORA MOVING JOC SOFTWARE PATH PASS"
echo "artifacts: $MOVING_DIR"
