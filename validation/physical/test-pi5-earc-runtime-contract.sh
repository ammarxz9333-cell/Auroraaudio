#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="$ROOT_DIR/config/external-components-v1.json"
LAYOUT="$ROOT_DIR/config/layouts/omniphony-11.1.4-aurora.yaml"
OVERLAY="$ROOT_DIR/platform/pi5/aurora-earc-tap-overlay.dts"
CONVERTER="$ROOT_DIR/validation/physical/aurora_alsa_iec61937_stream.py"

for script in \
  "$ROOT_DIR/scripts/pi5/build-runtime.sh" \
  "$ROOT_DIR/scripts/pi5/run-earc-joc.sh" \
  "$ROOT_DIR/scripts/pi5/install-earc-overlay.sh" \
  "$ROOT_DIR/scripts/pi5/check-health.sh" \
  "$ROOT_DIR/scripts/pi5/install-user-service.sh"; do
  bash -n "$script"
done
python3 -m py_compile "$CONVERTER" "$ROOT_DIR/scripts/pi5/make-camilladsp-config.py"

python3 - "$MANIFEST" "$LAYOUT" "$ROOT_DIR/scripts/pi5/run-earc-joc.sh" <<'PY'
import json, pathlib, re, sys
manifest=json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
components={c["id"]:c for c in manifest["components"]}
expected={
    "omniphony":("v0.6.0","dd5546bbc64e60719dfa367bea0534dc8a3ab34b"),
    "harletty-bridge":("v0.8.0","eddb123f876048268ee1f096ccdd1cbcf65ad07d"),
    "vibesboxsrc":("commit-8f84376df8b7499808b17c150a328b8665ba1384","8f84376df8b7499808b17c150a328b8665ba1384"),
    "camilladsp":("4.1.3","05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74"),
}
for cid,(version,commit) in expected.items():
    c=components.get(cid)
    if not c:
        raise SystemExit(f"missing component pin: {cid}")
    if c.get("tested_version") != version or c.get("pinned_commit") != commit:
        raise SystemExit(f"unexpected {cid} pin: {c.get('tested_version')} {c.get('pinned_commit')}")

cam=components["camilladsp"]
if cam.get("decision") != "adopted":
    raise SystemExit(f"CamillaDSP Pi5 deployment must be adopted, got {cam.get('decision')}")
if cam.get("integration") != "offline-rust-adapter-plus-external-process-pi5-realtime-post-dsp":
    raise SystemExit(f"unexpected CamillaDSP integration boundary: {cam.get('integration')}")
if cam.get("production_ready") is not False:
    raise SystemExit("CamillaDSP Pi5 path must remain non-production until physical validation")
arm=cam.get("release_artifacts",{}).get("aarch64-unknown-linux-gnu",{})
if arm.get("name") != "camilladsp-linux-aarch64.tar.gz":
    raise SystemExit("CamillaDSP ARM64 asset pin missing")
if arm.get("sha256") != "d9a17092923ebfe5d20a770c6b6a7eb2268f9700f999bf604b9db09f518aca5a":
    raise SystemExit("CamillaDSP ARM64 checksum pin mismatch")

text=pathlib.Path(sys.argv[2]).read_text(encoding="utf-8")
runtime=pathlib.Path(sys.argv[3]).read_text(encoding="utf-8")
if 'OUTPUT_MODE="${AURORA_OUTPUT_MODE:-camilladsp}"' not in runtime:
    raise SystemExit("CamillaDSP must be the default Pi5 output mode")
for token in ("--master-gain", "--auto-gain-ceiling", "--output-sample-rate"):
    if runtime.count(token) != 1:
        raise SystemExit(f"runtime option must appear exactly once: {token} count={runtime.count(token)}")
for token in ("--output-backend file", "--output-file -", "--output-file-format raw-f32"):
    if token not in runtime:
        raise SystemExit(f"CamillaDSP handoff missing runtime token: {token}")
if '"${GLOBAL_ARGS[@]}" render -' not in runtime:
    raise SystemExit("orender global config args must precede explicit render subcommand")
print("AURORA-PI5-RUNTIME-OPTIONS-CONTRACT-PASS output=camilladsp")

names=re.findall(r'^  - name: "([^"]+)"$', text, flags=re.M)
if len(names) != 16 or len(set(names)) != 16:
    raise SystemExit(f"Aurora layout must contain 16 unique outputs, got {len(names)}")
if names.count("LFE") != 1:
    raise SystemExit("Aurora layout must contain one LFE")
blocks=re.split(r'(?=^  - name: )', text, flags=re.M)
speaker_blocks={}
for block in blocks:
    match=re.search(r'^  - name: "([^"]+)"$', block, flags=re.M)
    if match:
        speaker_blocks[match.group(1)]=block

lfe=speaker_blocks.get("LFE","")
if "spatialize: true" not in lfe or "freq_high: 80" not in lfe:
    raise SystemExit("LFE must own the 0-80 Hz bass-management band")
floor={"FL","FR","C","FWL","FWR","SL","SR","RWL","RWR","BL","BR"}
heights={"TFL","TFR","TRL","TRR"}
for name in floor:
    if "freq_low: 80" not in speaker_blocks.get(name,""):
        raise SystemExit(f"{name} must be high-passed at 80 Hz")
for name in heights:
    if "freq_low: 100" not in speaker_blocks.get(name,""):
        raise SystemExit(f"{name} must be high-passed at 100 Hz")
if any("spatialize: false" in block for block in speaker_blocks.values()):
    raise SystemExit("bass-managed Aurora layout expects every output to participate in its declared frequency band")
print("AURORA-11.1.4-LAYOUT-CONTRACT-PASS channels=16 bass_management=80/100Hz")
PY

if command -v dtc >/dev/null 2>&1; then
  TMP_DTBO="$(mktemp)"
  trap 'rm -f "$TMP_DTBO"' EXIT
  dtc -@ -I dts -O dtb -o "$TMP_DTBO" "$OVERLAY"
  test -s "$TMP_DTBO"
  echo "AURORA-PI5-EARC-OVERLAY-COMPILE-PASS"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP" ${TMP_DTBO:-}' EXIT
mkdir -p "$TMP/bin"
cat > "$TMP/bin/arecord" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
if [[ " $* " == *" --dump-hw-params "* ]]; then
  echo "FORMAT: S32_LE"
  echo "CHANNELS: [2 8]"
  echo "RATE: [8000 384000]"
  exit 0
fi
python3 - "$AURORA_FAKE_ROOT" <<'PY'
import pathlib, sys
root=pathlib.Path(sys.argv[1])
sys.path.insert(0, str(root / "validation" / "physical"))
from aurora_alsa_iec61937_capture import synthetic_iec, canonical_to_s32
canonical=synthetic_iec(4)
sys.stdout.buffer.write(canonical_to_s32(canonical, word_lane="high16", channel_order="lr"))
PY
FAKE
chmod +x "$TMP/bin/arecord"

python3 - "$ROOT_DIR" "$TMP/expected.bin" <<'PY'
import pathlib, sys
root=pathlib.Path(sys.argv[1])
sys.path.insert(0, str(root / "validation" / "physical"))
from aurora_alsa_iec61937_capture import synthetic_iec
pathlib.Path(sys.argv[2]).write_bytes(synthetic_iec(4))
PY

AURORA_FAKE_ROOT="$ROOT_DIR" PATH="$TMP/bin:$PATH" python3 "$CONVERTER" capture   --device hw:eARC,0   --iec-out -   --status "$TMP/status.json"   --hw-params-log "$TMP/hw.txt"   --stderr-log "$TMP/arecord.txt"   >"$TMP/actual.bin" 2>"$TMP/runtime.stderr"

cmp "$TMP/expected.bin" "$TMP/actual.bin"
grep -q "AURORA-ALSA-IEC61937-STREAM-PASS" "$TMP/runtime.stderr"
python3 - "$TMP/actual.bin" "$TMP/status.json" <<'PY'
import json, pathlib, sys
data=pathlib.Path(sys.argv[1]).read_bytes()
status=json.loads(pathlib.Path(sys.argv[2]).read_text(encoding="utf-8"))
if not data.startswith(bytes.fromhex("72f81f4e")):
    raise SystemExit("stdout carrier lost IEC61937 sync")
if status.get("canonical_bytes") != len(data):
    raise SystemExit("stdout byte count does not match status")
if status.get("selected_word_lane") != "high16" or status.get("selected_channel_order") != "lr":
    raise SystemExit("fake ALSA layout lock mismatch")
print(f"AURORA-PI5-STDOUT-PIPE-CONTRACT-PASS bytes={len(data)}")
PY


cat > "$TMP/bin/orender" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" >"$AURORA_FAKE_ARGS_FILE"
cat >/dev/null
python3 - <<'PY'
import sys
# 256 frames of 16-channel F32 silence; enough to exercise the byte pipe.
sys.stdout.buffer.write(b"\x00" * (256 * 16 * 4))
PY
FAKE
chmod +x "$TMP/bin/orender"

cat > "$TMP/bin/camilladsp" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "--check" ]]; then
  test -f "${2:-}"
  exit 0
fi
printf '%s\n' "$@" >"$AURORA_FAKE_CAMILLA_ARGS_FILE"
cat >/dev/null
FAKE
chmod +x "$TMP/bin/camilladsp"

: >"$TMP/libharletty_bridge.so"

AURORA_FAKE_ROOT="$ROOT_DIR" \
AURORA_FAKE_ARGS_FILE="$TMP/orender.args" \
AURORA_FAKE_CAMILLA_ARGS_FILE="$TMP/camilla.args" \
AURORA_ORENDER="$TMP/bin/orender" \
AURORA_CAMILLADSP="$TMP/bin/camilladsp" \
AURORA_HARLETTY_BRIDGE="$TMP/libharletty_bridge.so" \
AURORA_SPEAKER_LAYOUT="$LAYOUT" \
AURORA_ALSA_OUTPUT_DEVICE="hw:AuroraTest,0" \
AURORA_STATE_DIR="$TMP/state" \
PATH="$TMP/bin:$PATH" \
  bash "$ROOT_DIR/scripts/pi5/run-earc-joc.sh" \
  >"$TMP/launcher.stdout" 2>"$TMP/launcher.stderr"

python3 - "$TMP/orender.args" "$TMP/camilla.args" "$TMP/state/camilladsp-runtime.yml" "$LAYOUT" <<'PY'
import json, pathlib, sys
orender=pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
camilla_args=pathlib.Path(sys.argv[2]).read_text(encoding="utf-8").splitlines()
cfg=json.loads(pathlib.Path(sys.argv[3]).read_text(encoding="utf-8"))
layout=sys.argv[4]

def require_pair(args, flag, value):
    try:
        i=args.index(flag)
    except ValueError:
        raise SystemExit(f"launcher missing {flag}")
    if i+1 >= len(args) or args[i+1] != value:
        got=args[i+1] if i+1 < len(args) else "<missing>"
        raise SystemExit(f"launcher {flag} expected {value!r}, got {got!r}")

if orender[:2] != ["render", "-"]:
    raise SystemExit(f"launcher must use explicit render stdin flow, got prefix {orender[:2]}")
require_pair(orender, "--bridge-path", pathlib.Path(sys.argv[1]).parent.joinpath("libharletty_bridge.so").as_posix())
require_pair(orender, "--speaker-layout", layout)
require_pair(orender, "--output-backend", "file")
require_pair(orender, "--output-file", "-")
require_pair(orender, "--output-file-format", "raw-f32")
require_pair(orender, "--output-sample-rate", "48000")
require_pair(orender, "--master-gain", "-3")
require_pair(orender, "--auto-gain-ceiling", "-1")
if orender.count("--auto-gain") != 1:
    raise SystemExit(f"launcher expected one --auto-gain, got {orender.count('--auto-gain')}")
if "--enable-adaptive-resampling" in orender:
    raise SystemExit("Omniphony adaptive resampling must be off in CamillaDSP output mode")

devices=cfg["devices"]
assert devices["samplerate"] == 48000
assert devices["chunksize"] == 512
assert devices["queuelimit"] == 2
assert devices["target_level"] == 512
assert devices["adjust_period"] == 3
assert devices["enable_rate_adjust"] is True
assert devices["resampler"] == {"type":"AsyncSinc","profile":"Balanced"}
assert devices["capture"] == {"type":"Stdin","channels":16,"format":"F32_LE"}
assert devices["playback"]["type"] == "Alsa"
assert devices["playback"]["channels"] == 16
assert devices["playback"]["device"] == "hw:AuroraTest,0"
if camilla_args != [sys.argv[3]]:
    raise SystemExit(f"CamillaDSP runtime args mismatch: {camilla_args}")
print("AURORA-PI5-CAMILLADSP-CONTRACT-PASS channels=16 rate_adjust=AsyncSinc chunk=512")
PY

echo "AURORA-PI5-EARC-RUNTIME-CONTRACT-PASS"
