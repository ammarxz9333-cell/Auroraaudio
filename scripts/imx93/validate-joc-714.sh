#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
DEPS_ROOT="${AURORA_DEPS_ROOT:-$HOME/aurora-deps}"
BRIDGE="${AURORA_HARLETTY_BRIDGE:-$DEPS_ROOT/harletty-bridge/target/release/libharletty_bridge.so}"
ORENDER="${AURORA_ORENDER:-$DEPS_ROOT/Omniphony/omniphony-renderer/target/release/orender}"
LAYOUT="${AURORA_SPEAKER_LAYOUT:-$REPO_ROOT/platforms/imx93/layouts/aurora-7.1.4.yaml}"
FIXTURE="${AURORA_JOC_FIXTURE:-$DEPS_ROOT/harletty-bridge/harletty/tests/fixtures/joc_atmos_1s.eac3}"

for path in "$BRIDGE" "$ORENDER" "$LAYOUT" "$FIXTURE"; do
  [[ -e "$path" ]] || { echo "missing integration-test input: $path" >&2; exit 2; }
done
command -v python3 >/dev/null 2>&1 || { echo 'python3 is required for output validation' >&2; exit 2; }

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
out="$tmpdir/joc-714.f32"
log="$tmpdir/orender.log"

# Exercise the same Raw stdin transport used by run-live-714.sh after Aurora has
# removed IEC61937. This is intentionally not a Harletty-only unit test: the
# decoded objects pass through the real Harletty bridge and the pinned Omniphony
# renderer/layout before file output.
cat "$FIXTURE" | "$ORENDER" - \
  --bridge-path "$BRIDGE" \
  --enable-vbap \
  --speaker-layout "$LAYOUT" \
  --output-backend file \
  --output-file "$out" \
  --output-file-format raw-f32 \
  >"$log" 2>&1

python3 - "$out" <<'PY'
import math
import os
import struct
import sys

path = sys.argv[1]
channels = 12
sample_rate = 48_000
frame_bytes = channels * 4
size = os.path.getsize(path)
if size <= 0:
    raise SystemExit("FAIL: renderer produced an empty file")
if size % frame_bytes:
    raise SystemExit(f"FAIL: {size} bytes is not divisible by a {channels}ch float32 frame ({frame_bytes} bytes)")
frames = size // frame_bytes
duration = frames / sample_rate
if not (0.50 <= duration <= 1.50):
    raise SystemExit(f"FAIL: unexpected rendered duration {duration:.6f}s ({frames} frames)")

peaks = [0.0] * channels
nonzero = [0] * channels
with open(path, "rb") as fh:
    while True:
        chunk = fh.read(frame_bytes * 1024)
        if not chunk:
            break
        if len(chunk) % 4:
            raise SystemExit("FAIL: truncated float32 sample")
        samples = struct.iter_unpack("<f", chunk)
        for index, (sample,) in enumerate(samples):
            if not math.isfinite(sample):
                raise SystemExit("FAIL: NaN/Inf in rendered output")
            channel = index % channels
            magnitude = abs(sample)
            peaks[channel] = max(peaks[channel], magnitude)
            if magnitude > 1e-8:
                nonzero[channel] += 1

if max(peaks) <= 1e-8:
    raise SystemExit("FAIL: rendered output is silent")

print(f"PASS: Harletty JOC -> Omniphony 7.1.4 produced {frames} frames ({duration:.6f}s), 12ch finite f32")
print("peaks=" + ",".join(f"{value:.8f}" for value in peaks))
print("nonzero_samples=" + ",".join(str(value) for value in nonzero))
PY

cat "$log"
