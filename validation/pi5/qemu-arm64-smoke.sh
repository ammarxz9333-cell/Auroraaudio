#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

arch="$(uname -m)"
[[ "$arch" == "aarch64" ]] || {
  echo "expected aarch64 QEMU userspace, got $arch" >&2
  exit 1
}
echo "AURORA-PI5-QEMU-ARCH-PASS arch=$arch"

dtc -@ -I dts -O dtb -o /tmp/aurora-earc-tap.dtbo platform/pi5/aurora-earc-tap-overlay.dts
test -s /tmp/aurora-earc-tap.dtbo
echo "AURORA-PI5-QEMU-OVERLAY-COMPILE-PASS"

python3 validation/physical/aurora_alsa_iec61937_capture.py self-test
python3 validation/physical/aurora_live_ingress.py self-test

cargo build --quiet --locked -p aurora-os-daemon
test -x target/debug/aurora-os-daemon
echo "AURORA-PI5-QEMU-RUST-BUILD-PASS binary=target/debug/aurora-os-daemon"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP" /tmp/aurora-earc-tap.dtbo' EXIT
mkdir -p "$TMP/modules"
cp os/aurora-os/examples/left-pod-detached.json "$TMP/modules/left-pod.json"
cp os/aurora-os/examples/right-pod-detached.json "$TMP/modules/right-pod.json"
cp os/aurora-os/presets/detached-atmos.json "$TMP/intent.json"

python3 - "$TMP" <<'PY'
import json, pathlib, sys
root=pathlib.Path(sys.argv[1])
cfg={
  "control":{
    "module_dir":str(root/"modules"),
    "intent_path":str(root/"intent.json"),
    "plan_path":str(root/"active-plan.json"),
    "health_path":str(root/"health.json"),
    "poll_millis":50,
    "policy":{
      "maximum_cinema_skew_micros":1000,
      "maximum_stereo_skew_micros":1000,
      "maximum_multiroom_skew_micros":10000
    }
  }
}
(root/"aurora-os.json").write_text(json.dumps(cfg), encoding="utf-8")
PY

target/debug/aurora-os-daemon --config "$TMP/aurora-os.json" --once

python3 - "$TMP/active-plan.json" "$TMP/health.json" <<'PY'
import json,sys
plan=json.load(open(sys.argv[1], encoding="utf-8"))
health=json.load(open(sys.argv[2], encoding="utf-8"))
assert health["state"] == "ready", health
assert health["assignment_count"] == 2, health
assert plan["mode"] == "cinema", plan
routes=[x["route"] for x in plan["assignments"]]
assert routes == ["wireless-network","wireless-network"], routes
roles=[x["roles"] for x in plan["assignments"]]
assert any("SurroundLeft" in x and "TopRearLeft" in x for x in roles), roles
assert any("SurroundRight" in x and "TopRearRight" in x for x in roles), roles
print("AURORA-PI5-QEMU-OS-PASS mode=cinema modules=2 routes=wireless-network roles=SL+TRL,SR+TRR")
PY

echo "AURORA-PI5-QEMU-SMOKE-PASS"
