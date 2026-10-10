#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN="${1:-$ROOT/target/aarch64-unknown-linux-gnu/debug/aurora-os-daemon}"

arch="$(uname -m)"
[[ "$arch" == "aarch64" ]] || {
  echo "expected aarch64 QEMU userspace, got $arch" >&2
  exit 1
}
echo "AURORA-PI5-QEMU-ARCH-PASS arch=$arch"

[[ -x "$BIN" ]] || {
  echo "ARM64 Aurora daemon missing: $BIN" >&2
  exit 1
}

TMP="/tmp/aurora-qemu-runtime"
rm -rf "$TMP"
mkdir -p "$TMP/modules"
trap 'rm -rf "$TMP"' EXIT

cp "$ROOT/os/aurora-os/examples/left-pod-detached.json" "$TMP/modules/left-pod.json"
cp "$ROOT/os/aurora-os/examples/right-pod-detached.json" "$TMP/modules/right-pod.json"
cp "$ROOT/os/aurora-os/presets/detached-atmos.json" "$TMP/intent.json"

cat > "$TMP/aurora-os.json" <<EOF
{
  "control": {
    "module_dir": "$TMP/modules",
    "intent_path": "$TMP/intent.json",
    "plan_path": "$TMP/active-plan.json",
    "health_path": "$TMP/health.json",
    "poll_millis": 50,
    "policy": {
      "maximum_cinema_skew_micros": 1000,
      "maximum_stereo_skew_micros": 1000,
      "maximum_multiroom_skew_micros": 10000
    }
  }
}
EOF

"$BIN" --config "$TMP/aurora-os.json" --once

grep -q '"state": "ready"' "$TMP/health.json"
grep -q '"assignment_count": 2' "$TMP/health.json"
grep -q '"mode": "cinema"' "$TMP/active-plan.json"
test "$(grep -c '"route": "wireless-network"' "$TMP/active-plan.json")" -eq 2
grep -q 'SurroundLeft' "$TMP/active-plan.json"
grep -q 'TopRearLeft' "$TMP/active-plan.json"
grep -q 'SurroundRight' "$TMP/active-plan.json"
grep -q 'TopRearRight' "$TMP/active-plan.json"

echo "AURORA-PI5-QEMU-OS-PASS mode=cinema modules=2 routes=wireless-network roles=SL+TRL,SR+TRR"
echo "AURORA-PI5-QEMU-SMOKE-PASS"
