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
    let mut bridge_error = false;

    // Match Harletty's raw-extractor tests: feed host-sized arbitrary chunks and
    // let the bridge own access-unit framing. Stream-level incompatibility is an
    // observed outcome, not a reason for the evidence harness itself to panic.
    for chunk in bytes.chunks(4096) {
        pushes += 1;
        let result = bridge.push_packet(chunk.into(), RInputTransport::Raw, 0);
        if !result.error_message.is_empty() {
            eprintln!(
                "HARLETTY-BRIDGE-ERROR push={pushes} message={}",
                result.error_message.as_str()
            );
            bridge_error = true;
            break;
        }
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

    let outcome = if bridge_error {
        "bridge-error"
    } else if frames == 0 {
        "no-frames"
    } else if metadata_frames == 0 {
        "pcm-only"
    } else if events == 0 || object_channels == 0 || !saw_objects {
        "metadata-incomplete"
    } else {
        "object-metadata"
    };

    println!(
        "{{\"pushes\":{pushes},\"frames\":{frames},\"metadata_frames\":{metadata_frames},\"events\":{events},\"object_channels\":{object_channels},\"resets\":{resets},\"saw_objects\":{saw_objects},\"bridge_error\":{bridge_error},\"outcome\":\"{outcome}\"}}"
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
    print(f"{vector['id']}\t{vector['output']}\t{vector['harletty_expectation']}")
PY

while IFS=$'\t' read -r vector_id output_name expectation; do
  input="$FIXTURE_DIR/$output_name"
  result="$OUTPUT_DIR/$vector_id.json"
  [[ -s "$input" ]] || fail "missing generated vector: $input"
  "$HARNESS_BIN" "$BRIDGE_LIB" "$input" > "$result"
  python3 -m json.tool "$result" >/dev/null
  outcome="$(python3 - "$result" <<'PY'
import json, sys
print(json.load(open(sys.argv[1], encoding='utf-8'))['outcome'])
PY
)"
  echo "JOCFORGE-HARLETTY-VECTOR-CLASSIFIED id=$vector_id expectation=$expectation outcome=$outcome"
done < "$WORK_DIR/vectors.tsv"

python3 - "$VECTOR_MANIFEST" "$OUTPUT_DIR" "$HARLETTY_COMMIT" <<'PY'
import json, pathlib, sys
manifest = json.load(open(sys.argv[1], encoding='utf-8'))
root = pathlib.Path(sys.argv[2])
harletty_commit = sys.argv[3]
allowed_outcomes = {'object-metadata', 'metadata-incomplete', 'pcm-only', 'no-frames', 'bridge-error'}
results = []
baseline_decoded = 0
object_metadata_vectors = 0
for vector in manifest['vectors']:
    path = root / f"{vector['id']}.json"
    payload = json.loads(path.read_text(encoding='utf-8'))
    expectation = vector.get('harletty_expectation')
    outcome = payload.get('outcome')
    if outcome not in allowed_outcomes:
        raise SystemExit(f"unclassified Harletty outcome for {vector['id']}: {payload}")
    if int(payload.get('pushes', 0)) <= 0:
        raise SystemExit(f"Harletty received no input for {vector['id']}: {payload}")

    if expectation == 'decode-classification-required':
        if payload.get('bridge_error') is True or int(payload.get('frames', 0)) <= 0:
            raise SystemExit(f"baseline Harletty decode failed for {vector['id']}: {payload}")
        baseline_decoded += 1
    elif expectation == 'bounded-classification-required':
        # These are intentionally minimal topology/conformance probes. The
        # pinned bridge may decode, reject, or expose no object metadata, but
        # every outcome must be explicit and the harness must terminate.
        pass
    else:
        raise SystemExit(f"unknown Harletty expectation for {vector['id']}: {expectation!r}")

    if outcome == 'object-metadata':
        object_metadata_vectors += 1
    results.append({
        'id': vector['id'],
        'source': vector['output'],
        'expectation': expectation,
        **payload,
    })

expected_baselines = sum(
    1 for vector in manifest['vectors']
    if vector.get('harletty_expectation') == 'decode-classification-required'
)
if baseline_decoded != expected_baselines:
    raise SystemExit(
        f"Harletty baseline decode coverage incomplete: {baseline_decoded}/{expected_baselines}"
    )

summary = {
    'schema_version': 2,
    'harletty_commit': harletty_commit,
    'input_transport': 'raw-eac3-host-sized-4096-byte-chunks',
    'vector_count': len(results),
    'all_vectors_classified': len(results) == manifest['vector_count'],
    'baseline_vectors_decoded': baseline_decoded,
    'baseline_vector_count': expected_baselines,
    'object_metadata_vectors': object_metadata_vectors,
    'results': results,
    'truth_boundary': 'This classifies the pinned Harletty bridge against the representative JOCForge corpus without converting an unsupported topology or absent metadata into a false pass. Baseline source-derived profile vectors must be accepted and emit decoded frames. Minimal structural probes may decode, emit PCM without object metadata, emit incomplete metadata, produce no frames, or be explicitly rejected; all such outcomes are retained as evidence. Object-metadata reconstruction is claimed only for vectors whose recorded outcome is object-metadata. Existing Aurora CI separately validates IEC61937 carriage. This does not prove renderer equivalence, exhaustive JOC coverage, physical eARC behavior, protected-service compatibility, proprietary equivalence, or certification.'
}
(root / 'jocforge-harletty-summary.json').write_text(
    json.dumps(summary, indent=2, sort_keys=True) + '\n',
    encoding='utf-8',
)
PY

echo "JOCFORGE-HARLETTY-MATRIX-PASS"
