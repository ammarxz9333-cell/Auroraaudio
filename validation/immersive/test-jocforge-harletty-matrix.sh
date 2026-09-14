#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="$ROOT_DIR/config/external-components-v1.json"
VECTOR_MANIFEST=${1:?usage: test-jocforge-harletty-matrix.sh VECTOR_MANIFEST FIXTURE_DIR OUTPUT_DIR}
FIXTURE_DIR=${2:?usage: test-jocforge-harletty-matrix.sh VECTOR_MANIFEST FIXTURE_DIR OUTPUT_DIR}
OUTPUT_DIR=${3:?usage: test-jocforge-harletty-matrix.sh VECTOR_MANIFEST FIXTURE_DIR OUTPUT_DIR}
TOOLCHAIN=${AURORA_EXTERNAL_RUST_TOOLCHAIN:-stable}
WORK_DIR=${AURORA_JOCFORGE_HARLETTY_WORKDIR:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-jocforge-harletty.XXXXXX")"}
KEEP_WORKDIR=${AURORA_KEEP_JOCFORGE_HARLETTY_WORKDIR:-0}

fail() {
  echo "JOCFORGE-HARLETTY-FAIL: $*" >&2
  exit 1
}

cleanup() {
  if [[ "$KEEP_WORKDIR" == "1" ]]; then
    echo "Keeping JOCForge/Harletty workdir: $WORK_DIR" >&2
  else
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

for cmd in cargo ffmpeg git python3 rustup; do
  command -v "$cmd" >/dev/null 2>&1 || fail "missing required command: $cmd"
done
[[ -f "$MANIFEST" ]] || fail "missing external component manifest"
[[ -f "$VECTOR_MANIFEST" ]] || fail "missing vector manifest: $VECTOR_MANIFEST"
[[ -d "$FIXTURE_DIR" ]] || fail "missing fixture directory: $FIXTURE_DIR"
mkdir -p "$WORK_DIR" "$OUTPUT_DIR"

# Resolve existing reviewed pins; never follow moving tags/branches silently.
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
  [[ "$actual" == "$commit" ]] || fail "pin mismatch for $url: expected $commit got $actual"
}

OMNIP_DIR="$WORK_DIR/Omniphony"
HARLETTY_DIR="$WORK_DIR/harletty-bridge"
TARGET_DIR="$WORK_DIR/harletty-target"
HARNESS_TARGET="$WORK_DIR/harness-target"
clone_pinned "$OMNIP_UPSTREAM" "$OMNIP_VERSION" "$OMNIP_COMMIT" "$OMNIP_DIR"
clone_pinned "$HARLETTY_UPSTREAM" "$HARLETTY_VERSION" "$HARLETTY_COMMIT" "$HARLETTY_DIR"
rustup toolchain install "$TOOLCHAIN" --profile minimal >/dev/null

CARGO_TARGET_DIR="$TARGET_DIR" cargo +"$TOOLCHAIN" build --locked --release \
  --manifest-path "$HARLETTY_DIR/Cargo.toml" -p harletty-bridge
BRIDGE_LIB="$TARGET_DIR/release/libharletty_bridge.so"
[[ -s "$BRIDGE_LIB" ]] || fail "Harletty bridge build product missing"

HARNESS_DIR="$WORK_DIR/harness"
mkdir -p "$HARNESS_DIR/src"
cat > "$HARNESS_DIR/Cargo.toml" <<EOF_CARGO
[package]
name = "aurora-jocforge-harletty-harness"
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
    let bridge_path = args.next().expect("bridge path");
    let carrier_path = args.next().expect("carrier path");
    assert!(args.next().is_none(), "unexpected extra arguments");

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
            assert_eq!(packet.data_type, 0x15, "expected E-AC-3 IEC61937 data type");
            let result = bridge.push_packet(
                packet.payload.as_slice().into(),
                RInputTransport::Iec61937,
                packet.data_type,
            );
            assert!(result.error_message.is_empty(), "Harletty bridge error: {}", result.error_message.as_str());
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
    assert!(metadata_frames > 0, "JOC vector emitted no metadata frames");
    assert!(events > 0, "JOC vector emitted no object events");
    assert!(object_channels > 0, "JOC vector emitted no object-channel declarations");
    assert!(saw_objects, "Harletty never reported objects");

    println!(
        "{{\"packets\":{packets},\"frames\":{frames},\"metadata_frames\":{metadata_frames},\"events\":{events},\"object_channels\":{object_channels},\"saw_objects\":true}}"
    );
}
EOF_RS

# Build once, then feed every generated vector through the exact same bridge/harness.
CARGO_TARGET_DIR="$HARNESS_TARGET" cargo +"$TOOLCHAIN" build --release --manifest-path "$HARNESS_DIR/Cargo.toml"
HARNESS_BIN="$HARNESS_TARGET/release/aurora-jocforge-harletty-harness"
[[ -x "$HARNESS_BIN" ]] || fail "matrix harness build product missing"

python3 - "$VECTOR_MANIFEST" <<'PY' > "$WORK_DIR/vectors.tsv"
import json, sys
manifest = json.load(open(sys.argv[1], encoding='utf-8'))
for vector in manifest['vectors']:
    print(f"{vector['id']}\t{vector['output']}")
PY

while IFS=$'\t' read -r vector_id output_name; do
  input="$FIXTURE_DIR/$output_name"
  carrier="$WORK_DIR/$vector_id.spdif"
  result="$OUTPUT_DIR/$vector_id.json"
  [[ -s "$input" ]] || fail "missing generated vector: $input"
  ffmpeg -nostdin -hide_banner -loglevel error -y -i "$input" -map 0:a:0 -c:a copy -f spdif "$carrier"
  [[ -s "$carrier" ]] || fail "failed to create IEC61937 carrier for $vector_id"
  "$HARNESS_BIN" "$BRIDGE_LIB" "$carrier" > "$result"
  python3 -m json.tool "$result" >/dev/null
  echo "JOCFORGE-HARLETTY-VECTOR-PASS id=$vector_id"
done < "$WORK_DIR/vectors.tsv"

python3 - "$VECTOR_MANIFEST" "$OUTPUT_DIR" <<'PY'
import json, pathlib, sys
manifest = json.load(open(sys.argv[1], encoding='utf-8'))
root = pathlib.Path(sys.argv[2])
results = []
for vector in manifest['vectors']:
    path = root / f"{vector['id']}.json"
    payload = json.loads(path.read_text(encoding='utf-8'))
    required = ['packets', 'frames', 'metadata_frames', 'events', 'object_channels']
    if not all(int(payload.get(key, 0)) > 0 for key in required) or payload.get('saw_objects') is not True:
        raise SystemExit(f"invalid Harletty evidence for {vector['id']}: {payload}")
    results.append({'id': vector['id'], 'source': vector['output'], **payload})
summary = {
    'schema_version': 1,
    'harletty_commit': '10943821cca7e6886c11f45d2267b06d76e6db7c',
    'vector_count': len(results),
    'all_vectors_decoded_with_object_metadata': len(results) == manifest['vector_count'],
    'results': results,
    'truth_boundary': 'This proves the pinned Harletty bridge decoded the representative JOCForge vectors through IEC61937 and exposed non-empty object metadata. It does not prove renderer equivalence, exhaustive JOC coverage, physical eARC behavior, protected-service compatibility, proprietary equivalence, or certification.'
}
(root / 'jocforge-harletty-summary.json').write_text(json.dumps(summary, indent=2, sort_keys=True) + '\n', encoding='utf-8')
PY

echo "JOCFORGE-HARLETTY-MATRIX-PASS"
