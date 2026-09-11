#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
MANIFEST="$ROOT_DIR/config/external-components-v1.json"
OMNIP_PATCH="$ROOT_DIR/platform/s6/patches/omniphony-raw-f32-low-latency.patch"
TOOLCHAIN="${AURORA_EXTERNAL_RUST_TOOLCHAIN:-stable}"
KEEP_WORKDIR="${AURORA_KEEP_JOC_TEST_WORKDIR:-0}"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This validation currently targets the Linux S6 software path." >&2
  exit 2
fi
for cmd in git python3 ffmpeg rustup cargo; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
[[ -f "$MANIFEST" ]] || { echo "missing external component manifest: $MANIFEST" >&2; exit 2; }
[[ -f "$OMNIP_PATCH" ]] || { echo "missing Omniphony latency patch: $OMNIP_PATCH" >&2; exit 2; }

WORK_DIR="${AURORA_JOC_TEST_WORKDIR:-$(mktemp -d "${TMPDIR:-/tmp}/aurora-joc-stack.XXXXXX")}" 
mkdir -p "$WORK_DIR"
cleanup() {
  if [[ "$KEEP_WORKDIR" == "1" ]]; then
    echo "Keeping JOC validation workdir: $WORK_DIR"
  else
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

# Read the exact tested tags and commits from Aurora's manifest so this gate
# cannot silently validate a different upstream revision.
eval "$(python3 - "$MANIFEST" <<'PY'
import json, shlex, sys
manifest = json.load(open(sys.argv[1], encoding='utf-8'))
components = {c['id']: c for c in manifest['components']}
for prefix, cid in [('OMNIP', 'omniphony'), ('HARLETTY', 'harletty-bridge')]:
    c = components[cid]
    for key, value in [
        ('UPSTREAM', c['upstream']),
        ('VERSION', c['tested_version']),
        ('COMMIT', c['pinned_commit']),
    ]:
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
clone_pinned "$OMNIP_UPSTREAM" "$OMNIP_VERSION" "$OMNIP_COMMIT" "$OMNIP_DIR"
clone_pinned "$HARLETTY_UPSTREAM" "$HARLETTY_VERSION" "$HARLETTY_COMMIT" "$HARLETTY_DIR"

# Harletty 0.7.4 declares rust-version 1.87; keep this external validation off
# Aurora's MSRV toolchain by using an explicit modern toolchain.
rustup toolchain install "$TOOLCHAIN" --profile minimal >/dev/null

# 1) Establish that the pinned real fixture still decodes as E-AC-3 JOC and
# produces byte-pinned object metadata/audio in Harletty's own golden test.
cargo +"$TOOLCHAIN" test --locked \
  --manifest-path "$HARLETTY_DIR/Cargo.toml" \
  -p harletty --test golden joc_master_set_matches_golden -- --exact --nocapture

# 2) Build the exact runtime plugin against Aurora's pinned Omniphony ABI.
cargo +"$TOOLCHAIN" build --locked --release \
  --manifest-path "$HARLETTY_DIR/Cargo.toml" \
  -p harletty-bridge

# 3) Apply Aurora's pinned low-latency raw-f32 sink patch and build orender.
git -C "$OMNIP_DIR/omniphony-renderer" apply --check "$OMNIP_PATCH"
git -C "$OMNIP_DIR/omniphony-renderer" apply "$OMNIP_PATCH"
cargo +"$TOOLCHAIN" build --locked --release \
  --manifest-path "$OMNIP_DIR/omniphony-renderer/Cargo.toml" \
  -p omniphony-renderer

JOC_FIXTURE="$HARLETTY_DIR/harletty/tests/fixtures/joc_atmos_1s.eac3"
IEC_FILE="$WORK_DIR/joc_atmos_1s.spdif"
BRIDGE_LIB="$HARLETTY_DIR/target/release/libharletty_bridge.so"
ORENDER="$OMNIP_DIR/omniphony-renderer/target/release/orender"
LAYOUT="$OMNIP_DIR/layouts/7.1.4.yaml"
RENDER_OUT="$WORK_DIR/joc_atmos_7_1_4.f32"
RENDER_LOG="$WORK_DIR/orender-joc.log"

for path in "$JOC_FIXTURE" "$BRIDGE_LIB" "$ORENDER" "$LAYOUT"; do
  [[ -f "$path" ]] || { echo "expected validation input/build product missing: $path" >&2; exit 1; }
done

# FFmpeg's IEC61937 muxer writes E-AC-3 length codes in bytes, matching the
# pinned Omniphony SpdifParser contract for data type 0x15.
ffmpeg -nostdin -hide_banner -loglevel error -y \
  -i "$JOC_FIXTURE" -map 0:a:0 -c:a copy -f spdif "$IEC_FILE"

python3 - "$IEC_FILE" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
data = p.read_bytes()
if len(data) < 8:
    raise SystemExit("IEC61937 carrier is too short")
if data[:4] != bytes.fromhex("72f81f4e"):
    raise SystemExit(f"unexpected IEC61937 sync: {data[:4].hex()}")
if data[4] & 0x1f != 0x15:
    raise SystemExit(f"expected E-AC-3 IEC61937 data type 0x15, got 0x{data[4] & 0x1f:02x}")
pd = int.from_bytes(data[6:8], "little")
if pd <= 0:
    raise SystemExit("first E-AC-3 burst has an empty Pd length")
print(f"IEC61937 carrier PASS: bytes={len(data)} first_pd={pd} data_type=0x15")
PY

# 4) Exercise the production transport contract directly: arbitrary carrier
# chunks -> Omniphony SpdifParser -> Harletty push_packet(Iec61937, 0x15).
# The gate requires actual object telemetry, not merely decoded multichannel PCM.
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
    assert!(args.next().is_none(), "unexpected extra arguments");

    let lib = BridgeLibRef::load_from_file(Path::new(&bridge_path))
        .expect("load Harletty bridge");
    let mut bridge = (lib.new_bridge())(false);
    let carrier = fs::read(&carrier_path).expect("read IEC61937 carrier");
    let mut parser = SpdifParser::new();

    let mut packets = 0usize;
    let mut frames = 0usize;
    let mut metadata_frames = 0usize;
    let mut events = 0usize;
    let mut object_channels = 0usize;
    let mut saw_objects = false;

    // Odd chunk size deliberately cuts across IEC61937 headers/payloads.
    for chunk in carrier.chunks(997) {
        parser.push_bytes(chunk);
        while let Some(packet) = parser.get_next_packet() {
            packets += 1;
            assert_eq!(packet.data_type, 0x15, "non-E-AC-3 burst in JOC fixture");
            let result = bridge.push_packet(
                packet.payload.as_slice().into(),
                RInputTransport::Iec61937,
                packet.data_type,
            );
            assert!(
                result.error_message.is_empty(),
                "bridge error after packet {packets}: {}",
                result.error_message
            );
            frames += result.frames.len();
            for frame in result.frames.iter() {
                if !frame.metadata.is_empty() {
                    metadata_frames += 1;
                }
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
    assert!(metadata_frames > 0, "JOC fixture emitted no metadata frames");
    assert!(events > 0, "JOC fixture emitted no object events");
    assert!(object_channels > 0, "JOC fixture emitted no object-to-channel declarations");
    assert!(saw_objects, "bridge.has_objects() never became true");

    println!(
        "JOC-IEC61937-PASS packets={packets} frames={frames} metadata_frames={metadata_frames} events={events} object_channels={object_channels}"
    );
}
EOF_RS
cargo +"$TOOLCHAIN" run --quiet --release --manifest-path "$HARNESS_DIR/Cargo.toml" -- \
  "$BRIDGE_LIB" "$IEC_FILE"

# 5) Feed the same IEC61937 JOC carrier through the actual orender CLI and the
# same runtime bridge, rendering to the pinned 7.1.4 speaker layout.
RUST_LOG="${RUST_LOG:-info}" "$ORENDER" "$IEC_FILE" \
  --bridge-path "$BRIDGE_LIB" \
  --enable-vbap \
  --speaker-layout "$LAYOUT" \
  --output-backend file \
  --output-file "$RENDER_OUT" \
  --output-file-format raw-f32 \
  >"$RENDER_LOG" 2>&1

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

echo "AURORA JOC SOFTWARE STACK PASS"
