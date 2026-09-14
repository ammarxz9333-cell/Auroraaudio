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

for cmd in cargo git python3 rustup; do
  command -v "$cmd" >/dev/null 2>&1 || fail "missing required command: $cmd"
done
[[ -f "$MANIFEST" ]] || fail "missing external component manifest"
[[ -f "$VECTOR_MANIFEST" ]] || fail "missing vector manifest: $VECTOR_MANIFEST"
[[ -d "$FIXTURE_DIR" ]] || fail "missing fixture directory: $FIXTURE_DIR"
mkdir -p "$WORK_DIR" "$OUTPUT_DIR"

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
EOF_CARGO

cat > "$HARNESS_DIR/src/main.rs" <<'EOF_RS'
use abi_stable::library::RootModule;
use bridge_api::{BridgeLibRef, RInputTransport};
use std::{env, fs, path::Path};

fn main() {
    let mut args = env::args().skip(1);
    let bridge_path = args.next().expect("bridge path");
    let input_path = args.next().expect("raw E-AC-3/JOC path");
    assert!(args.next().is_none(), "unexpected extra arguments");

    let lib = BridgeLibRef::load_from_file(Path::new(&bridge_path)).expect("load Harletty bridge");
    let mut bridge = (lib.new_bridge())(false);
    let bytes = fs::read(&input_path).expect("read raw E-AC-3/JOC input");
    assert!(!bytes.is_empty(), "raw input is empty");

    let mut pushes = 0usize;
    let mut frames = 0usize;
    let mut metadata_frames = 0usize;
    let mut events = 0usize;
    let mut object_channels = 0usize;
    let mut saw_objects = false;
    let mut resets = 0usize;

    // Match Harletty's own raw-extractor tests: feed host-sized arbitrary chunks
    // and let the bridge own access-unit framing. This preserves JOCForge bytes
    // exactly and avoids a demux/remux tool changing dependent substreams.
    for chunk in bytes.chunks(4096) {
        pushes += 1;
        let result = bridge.push_packet(chunk.into(), RInputTransport::Raw, 0);
        assert!(
            result.error_message.is_empty(),
            "Harletty bridge error after push {pushes}: {}",
            result.error_message.as_str()
        );
        if result.did_reset {
            resets += 1;
        }
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

    assert!(pushes > 0, "no raw chunks submitted");
    assert!(frames > 0, "Harletty emitted no decoded frames");
    assert!(metadata_frames > 0, "JOC vector emitted no metadata frames");
    assert!(events > 0, "JOC vector emitted no object events");
    assert!(object_channels > 0, "JOC vector emitted no object-channel declarations");
    assert!(saw_objects, "Harletty never reported objects");

    println!(
        "{{\"pushes\":{pushes},\"frames\":{frames},\"metadata_frames\":{metadata_frames},\"events\":{events},\"object_channels\":{object_channels},\"resets\":{resets},\"saw_objects\":true}}"
    );
}
EOF_RS

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
  result="$OUTPUT_DIR/$vector_id.json"
  [[ -s "$input" ]] || fail "missing generated vector: $input"
  "$HARNESS_BIN" "$BRIDGE_LIB" "$input" > "$result"
  python3 -m json.tool "$result" >/dev/null
  echo "JOCFORGE-HARLETTY-VECTOR-PASS id=$vector_id"
done < "$WORK_DIR/vectors.tsv"

python3 - "$VECTOR_MANIFEST" "$OUTPUT_DIR" "$HARLETTY_COMMIT" <<'PY'
import json, pathlib, sys
manifest = json.load(open(sys.argv[1], encoding='utf-8'))
root = pathlib.Path(sys.argv[2])
harletty_commit = sys.argv[3]
results = []
for vector in manifest['vectors']:
    path = root / f"{vector['id']}.json"
    payload = json.loads(path.read_text(encoding='utf-8'))
    required = ['pushes', 'frames', 'metadata_frames', 'events', 'object_channels']
    if not all(int(payload.get(key, 0)) > 0 for key in required) or payload.get('saw_objects') is not True:
        raise SystemExit(f"invalid Harletty evidence for {vector['id']}: {payload}")
    results.append({'id': vector['id'], 'source': vector['output'], **payload})
summary = {
    'schema_version': 1,
    'harletty_commit': harletty_commit,
    'input_transport': 'raw-eac3-host-sized-4096-byte-chunks',
    'vector_count': len(results),
    'all_vectors_decoded_with_object_metadata': len(results) == manifest['vector_count'],
    'results': results,
    'truth_boundary': 'This proves the pinned Harletty bridge decoded the representative JOCForge raw vectors and exposed non-empty object metadata while owning access-unit framing. Existing Aurora CI separately validates IEC61937 carriage. This does not prove renderer equivalence, exhaustive JOC coverage, physical eARC behavior, protected-service compatibility, proprietary equivalence, or certification.'
}
(root / 'jocforge-harletty-summary.json').write_text(json.dumps(summary, indent=2, sort_keys=True) + '\n', encoding='utf-8')
PY

echo "JOCFORGE-HARLETTY-MATRIX-PASS"
