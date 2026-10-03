#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="$ROOT_DIR/config/external-components-v1.json"
OMNIP_ARM64_PATCH="$ROOT_DIR/validation/immersive/omniphony-v0.6.0-arm64-c-char.patch"
TOOLCHAIN="${AURORA_EXTERNAL_RUST_TOOLCHAIN:-stable}"
KEEP_WORKDIR="${AURORA_KEEP_JOC_TEST_WORKDIR:-0}"
BUILD_MODE="${AURORA_JOC_BUILD_MODE:-debug}"

phase() {
  printf '\n== Aurora JOC phase: %s ==\n' "$1"
  date -u '+%Y-%m-%dT%H:%M:%SZ'
}

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

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This validation currently targets the Linux software validation path." >&2
  exit 2
fi
for cmd in git python3 ffmpeg rustup cargo; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
[[ -f "$MANIFEST" ]] || { echo "missing external component manifest: $MANIFEST" >&2; exit 2; }
[[ -f "$OMNIP_ARM64_PATCH" ]] || { echo "missing Omniphony ARM64 portability patch: $OMNIP_ARM64_PATCH" >&2; exit 2; }

if [[ -n "${AURORA_JOC_TEST_WORKDIR:-}" ]]; then
  WORK_DIR="$AURORA_JOC_TEST_WORKDIR"
  rm -rf "$WORK_DIR"
  mkdir -p "$WORK_DIR"
else
  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/aurora-joc-stack.XXXXXX")"
fi
cleanup() {
  if [[ "$KEEP_WORKDIR" == "1" ]]; then
    echo "Keeping JOC validation workdir: $WORK_DIR"
  else
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

eval "$(python3 - "$MANIFEST" <<'PY'
import json, shlex, sys
manifest = json.load(open(sys.argv[1], encoding='utf-8'))
components = {c['id']: c for c in manifest['components']}
for prefix, cid in [('OMNIP', 'omniphony'), ('HARLETTY', 'harletty-bridge')]:
    c = components[cid]
    for key, value in [('UPSTREAM', c['upstream']), ('VERSION', c['tested_version']), ('COMMIT', c['pinned_commit'])]:
        print(f"{prefix}_{key}={shlex.quote(value)}")
PY
)"

clone_pinned() {
  local url="$1" version="$2" commit="$3" dest="$4"
  git clone --quiet --depth 1 --branch "$version" "$url" "$dest"
  local actual
  actual="$(git -C "$dest" rev-parse HEAD)"
  if [[ "$actual" != "$commit" ]]; then
    echo "pinned commit mismatch for $url: expected $commit, got $actual" >&2
    exit 1
  fi
}

OMNIP_DIR="$WORK_DIR/Omniphony"
HARLETTY_DIR="$WORK_DIR/harletty-bridge"
HARLETTY_TARGET_DIR="${AURORA_HARLETTY_TARGET_DIR:-$HARLETTY_DIR/target}"
OMNIP_TARGET_DIR="${AURORA_OMNIP_TARGET_DIR:-$OMNIP_DIR/omniphony-renderer/target}"
HARNESS_TARGET_DIR="${AURORA_JOC_HARNESS_TARGET_DIR:-$WORK_DIR/iec-joc-harness-target}"
mkdir -p "$HARLETTY_TARGET_DIR" "$OMNIP_TARGET_DIR" "$HARNESS_TARGET_DIR"

phase "clone pinned external components"
clone_pinned "$OMNIP_UPSTREAM" "$OMNIP_VERSION" "$OMNIP_COMMIT" "$OMNIP_DIR"
clone_pinned "$HARLETTY_UPSTREAM" "$HARLETTY_VERSION" "$HARLETTY_COMMIT" "$HARLETTY_DIR"

phase "install external Rust toolchain"
rustup toolchain install "$TOOLCHAIN" --profile minimal >/dev/null

phase "run Harletty official JOC golden fixture test"
CARGO_TARGET_DIR="$HARLETTY_TARGET_DIR" cargo +"$TOOLCHAIN" test --locked \
  --manifest-path "$HARLETTY_DIR/Cargo.toml" \
  -p harletty --test golden joc_master_set_matches_golden -- --exact --nocapture

phase "build Harletty bridge ($BUILD_MODE)"
CARGO_TARGET_DIR="$HARLETTY_TARGET_DIR" cargo +"$TOOLCHAIN" build --locked "${PROFILE_ARGS[@]}" \
  --manifest-path "$HARLETTY_DIR/Cargo.toml" \
  -p harletty-bridge

phase "apply Omniphony 0.6 c_char portability patch"
git -C "$OMNIP_DIR" apply --check "$OMNIP_ARM64_PATCH"
git -C "$OMNIP_DIR" apply "$OMNIP_ARM64_PATCH"

phase "build Omniphony renderer ($BUILD_MODE)"
CARGO_TARGET_DIR="$OMNIP_TARGET_DIR" cargo +"$TOOLCHAIN" build "${PROFILE_ARGS[@]}" \
  --manifest-path "$OMNIP_DIR/omniphony-renderer/Cargo.toml" \
  -p omniphony-renderer

JOC_FIXTURE="$HARLETTY_DIR/harletty/tests/fixtures/joc_atmos_1s.eac3"
IEC_FILE="$WORK_DIR/joc_atmos_1s.spdif"
PLAIN_IEC_FILE="$WORK_DIR/plain_eac3_5_1.spdif"
BRIDGE_LIB="$HARLETTY_TARGET_DIR/$PROFILE_DIR/libharletty_bridge.so"
ORENDER="$OMNIP_TARGET_DIR/$PROFILE_DIR/orender"
LAYOUT="$OMNIP_DIR/layouts/7.1.4.yaml"
AURORA_LAYOUT="$ROOT_DIR/config/layouts/omniphony-11.1.4-aurora.yaml"
RENDER_OUT="$WORK_DIR/joc_atmos_7_1_4.f32"
AURORA_RENDER_OUT="$WORK_DIR/joc_atmos_aurora_11_1_4.f32"
RENDER_LOG="$WORK_DIR/orender-joc.log"
AURORA_RENDER_LOG="$WORK_DIR/orender-joc-aurora-11.1.4.log"
AURORA_STDOUT_RENDER="$WORK_DIR/joc_atmos_aurora_11_1_4_stdout.f32"
AURORA_STDOUT_LOG="$WORK_DIR/orender-joc-aurora-11.1.4-stdout.log"

for path in "$JOC_FIXTURE" "$BRIDGE_LIB" "$ORENDER" "$LAYOUT" "$AURORA_LAYOUT"; do
  [[ -f "$path" ]] || { echo "expected validation input/build product missing: $path" >&2; exit 1; }
done

phase "create and verify IEC61937 carriers"
ffmpeg -nostdin -hide_banner -loglevel error -y \
  -i "$JOC_FIXTURE" -map 0:a:0 -c:a copy -f spdif "$IEC_FILE"

ffmpeg -nostdin -hide_banner -loglevel error -y \
  -f lavfi -i "anullsrc=channel_layout=5.1:sample_rate=48000" \
  -t 0.25 -c:a eac3 -b:a 448k -f spdif "$PLAIN_IEC_FILE"

python3 - "$IEC_FILE" "$PLAIN_IEC_FILE" <<'PY'
import pathlib, sys
for filename in sys.argv[1:]:
    p = pathlib.Path(filename)
    data = p.read_bytes()
    if len(data) < 8:
        raise SystemExit(f"IEC61937 carrier is too short: {p}")
    if data[:4] != bytes.fromhex("72f81f4e"):
        raise SystemExit(f"unexpected IEC61937 sync in {p}: {data[:4].hex()}")
    if data[4] & 0x1f != 0x15:
        raise SystemExit(f"expected E-AC-3 IEC61937 data type 0x15 in {p}, got 0x{data[4] & 0x1f:02x}")
    pd = int.from_bytes(data[6:8], "little")
    if pd <= 0:
        raise SystemExit(f"first E-AC-3 burst has an empty Pd length: {p}")
    print(f"IEC61937 carrier PASS: file={p.name} bytes={len(data)} first_pd={pd} data_type=0x15")
PY

HARNESS_DIR="$WORK_DIR/iec-joc-harness"
mkdir -p "$HARNESS_DIR/src"
cat > "$HARNESS_DIR/Cargo.toml" <<EOF_CARGO
[package]
name = "aurora-iec-joc-harness"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
abi_stable = "0.11"
bridge_api = { path = "$OMNIP_DIR/omniphony-renderer/bridge_api" }
spdif = { path = "$OMNIP_DIR/omniphony-renderer/spdif" }
EOF_CARGO
cat > "$HARNESS_DIR/src/main.rs" <<'EOF_RS'
use abi_stable::library::RootModule;
use bridge_api::{BridgeLibRef, RInputTransport};
use spdif::SpdifParser;
use std::{env, fs, path::Path};

fn main() {
    let mut args = env::args().skip(1);
    let bridge_path = args.next().expect("bridge path argument");
    let carrier_path = args.next().expect("IEC61937 carrier argument");
    let expectation = args.next().expect("expectation argument: objects|plain");
    assert!(args.next().is_none(), "unexpected extra arguments");
    let expect_objects = match expectation.as_str() {
        "objects" => true,
        "plain" => false,
        other => panic!("unknown expectation: {other}"),
    };

    let lib = BridgeLibRef::load_from_file(Path::new(&bridge_path)).expect("load Harletty bridge");
    let mut bridge = (lib.new_bridge())(false);
    let carrier = fs::read(&carrier_path).expect("read IEC61937 carrier");
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
            assert_eq!(packet.data_type, 0x15, "non-E-AC-3 burst in fixture");
            let result = bridge.push_packet(
                packet.payload.as_slice().into(),
                RInputTransport::Iec61937,
                packet.data_type,
            );
            assert!(result.error_message.is_empty(), "bridge error after packet {packets}: {}", result.error_message.as_str());
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

    assert!(packets > 0, "no IEC61937 packets extracted");
    assert!(frames > 0, "Harletty emitted no decoded frames");

    if expect_objects {
        assert!(metadata_frames > 0, "JOC fixture emitted no metadata frames");
        assert!(events > 0, "JOC fixture emitted no object events");
        assert!(object_channels > 0, "JOC fixture emitted no object-to-channel declarations");
        assert!(saw_objects, "bridge.has_objects() never became true for JOC");
        println!("JOC-IEC61937-PASS packets={packets} frames={frames} metadata_frames={metadata_frames} events={events} object_channels={object_channels}");
    } else {
        assert!(!saw_objects, "plain E-AC-3 incorrectly reported JOC objects");
        println!("PLAIN-EAC3-NEGATIVE-CONTROL-PASS packets={packets} frames={frames} metadata_frames={metadata_frames} events={events} object_channels={object_channels}");
    }
}
EOF_RS

phase "run plain E-AC-3 negative control through real bridge"
CARGO_TARGET_DIR="$HARNESS_TARGET_DIR" cargo +"$TOOLCHAIN" run --quiet "${PROFILE_ARGS[@]}" --manifest-path "$HARNESS_DIR/Cargo.toml" -- \
  "$BRIDGE_LIB" "$PLAIN_IEC_FILE" plain

phase "run real JOC IEC61937 object-metadata harness"
CARGO_TARGET_DIR="$HARNESS_TARGET_DIR" cargo +"$TOOLCHAIN" run --quiet "${PROFILE_ARGS[@]}" --manifest-path "$HARNESS_DIR/Cargo.toml" -- \
  "$BRIDGE_LIB" "$IEC_FILE" objects

phase "render real JOC IEC61937 fixture to 7.1.4"
if ! RUST_LOG="${RUST_LOG:-info}" "$ORENDER" "$IEC_FILE" \
  --bridge-path "$BRIDGE_LIB" \
  --enable-vbap \
  --speaker-layout "$LAYOUT" \
  --output-backend file \
  --output-file "$RENDER_OUT" \
  --output-file-format raw-f32 \
  >"$RENDER_LOG" 2>&1; then
  echo "orender failed while rendering the real JOC IEC61937 fixture:" >&2
  cat "$RENDER_LOG" >&2
  exit 1
fi

python3 - "$RENDER_OUT" <<'PY'
from array import array
import math, pathlib, sys
p = pathlib.Path(sys.argv[1])
data = p.read_bytes()
frame_bytes = 12 * 4
if len(data) < frame_bytes * 100:
    raise SystemExit(f"7.1.4 render output too short: {len(data)} bytes")
if len(data) % frame_bytes:
    raise SystemExit(f"render output is not whole 12-channel f32 frames: {len(data)} bytes")
samples = array('f')
samples.frombytes(data)
if not all(math.isfinite(v) for v in samples):
    raise SystemExit("render output contains NaN/Inf")
if not any(abs(v) > 1e-8 for v in samples):
    raise SystemExit("render output is silent")
frames = len(samples) // 12
rms = []
for ch in range(12):
    vals = samples[ch::12]
    rms.append(math.sqrt(sum(v * v for v in vals) / max(1, len(vals))))
print(f"7.1.4 render PASS: frames={frames} bytes={len(data)} channel_rms=" + ",".join(f"{v:.6g}" for v in rms))
PY

phase "render real JOC IEC61937 fixture to Aurora custom 11.1.4"
if ! RUST_LOG="${RUST_LOG:-info}" "$ORENDER" "$IEC_FILE" \
  --bridge-path "$BRIDGE_LIB" \
  --enable-vbap \
  --speaker-layout "$AURORA_LAYOUT" \
  --output-backend file \
  --output-file "$AURORA_RENDER_OUT" \
  --output-file-format raw-f32 \
  >"$AURORA_RENDER_LOG" 2>&1; then
  echo "orender failed while rendering Aurora custom 11.1.4:" >&2
  cat "$AURORA_RENDER_LOG" >&2
  exit 1
fi

python3 - "$AURORA_RENDER_OUT" <<'PY'
from array import array
import math, pathlib, sys
p = pathlib.Path(sys.argv[1])
data = p.read_bytes()
channels = 16
frame_bytes = channels * 4
if len(data) < frame_bytes * 100:
    raise SystemExit(f"Aurora 11.1.4 render output too short: {len(data)} bytes")
if len(data) % frame_bytes:
    raise SystemExit(f"Aurora 11.1.4 output is not whole 16-channel f32 frames: {len(data)} bytes")
samples = array('f')
samples.frombytes(data)
if not all(math.isfinite(v) for v in samples):
    raise SystemExit("Aurora 11.1.4 render output contains NaN/Inf")
if not any(abs(v) > 1e-8 for v in samples):
    raise SystemExit("Aurora 11.1.4 render output is silent")
frames = len(samples) // channels
rms = []
for ch in range(channels):
    vals = samples[ch::channels]
    rms.append(math.sqrt(sum(v * v for v in vals) / max(1, len(vals))))
active = sum(v > 1e-8 for v in rms)
if active < 2:
    raise SystemExit(f"Aurora 11.1.4 render has too few active channels: {active}")
print(
    f"AURORA-11.1.4-JOC-RENDER-PASS frames={frames} bytes={len(data)} active_channels={active} "
    + "channel_rms=" + ",".join(f"{v:.6g}" for v in rms)
)
PY

phase "render Aurora 11.1.4 as raw F32 on stdout"
if ! RUST_LOG="${RUST_LOG:-info}" "$ORENDER" "$IEC_FILE" \
  --bridge-path "$BRIDGE_LIB" \
  --enable-vbap \
  --speaker-layout "$AURORA_LAYOUT" \
  --output-backend file \
  --output-file - \
  --output-file-format raw-f32 \
  >"$AURORA_STDOUT_RENDER" 2>"$AURORA_STDOUT_LOG"; then
  echo "orender failed while emitting Aurora raw F32 to stdout:" >&2
  cat "$AURORA_STDOUT_LOG" >&2
  exit 1
fi

python3 - "$AURORA_RENDER_OUT" "$AURORA_STDOUT_RENDER" <<'PY'
from array import array
import math, pathlib, sys

file_path = pathlib.Path(sys.argv[1])
stdout_path = pathlib.Path(sys.argv[2])
file_data = file_path.read_bytes()
stdout_data = stdout_path.read_bytes()
channels = 16
frame_bytes = channels * 4

if not stdout_data:
    raise SystemExit("Aurora stdout F32 render is empty")
if len(stdout_data) % frame_bytes:
    raise SystemExit(
        f"Aurora stdout F32 is contaminated/misaligned: {len(stdout_data)} bytes"
    )
samples = array("f")
samples.frombytes(stdout_data)
if not all(math.isfinite(v) for v in samples):
    raise SystemExit("Aurora stdout F32 contains NaN/Inf")
if not any(abs(v) > 1e-8 for v in samples):
    raise SystemExit("Aurora stdout F32 render is silent")

file_frames = len(file_data) // frame_bytes
stdout_frames = len(stdout_data) // frame_bytes
if stdout_frames != file_frames:
    raise SystemExit(
        f"stdout/file frame mismatch: stdout={stdout_frames} file={file_frames}"
    )

print(
    f"AURORA-11.1.4-STDOUT-F32-PASS "
    f"frames={stdout_frames} bytes={len(stdout_data)} channels={channels}"
)
PY

echo "AURORA JOC SOFTWARE STACK PASS"
