#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="$ROOT_DIR/config/external-components-v1.json"
LAYOUT="$ROOT_DIR/config/layouts/omniphony-11.1.4-aurora.yaml"
OVERLAY="$ROOT_DIR/platform/pi5/aurora-earc-tap-overlay.dts"
CONVERTER="$ROOT_DIR/validation/physical/aurora_alsa_iec61937_stream.py"

for script in   "$ROOT_DIR/scripts/pi5/build-runtime.sh"   "$ROOT_DIR/scripts/pi5/run-earc-joc.sh"   "$ROOT_DIR/scripts/pi5/install-earc-overlay.sh"; do
  bash -n "$script"
done
python3 -m py_compile "$CONVERTER"

python3 - "$MANIFEST" "$LAYOUT" <<'PY'
import json, pathlib, re, sys
manifest=json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
components={c["id"]:c for c in manifest["components"]}
expected={
    "omniphony":("v0.6.0","dd5546bbc64e60719dfa367bea0534dc8a3ab34b"),
    "harletty-bridge":("v0.8.0","eddb123f876048268ee1f096ccdd1cbcf65ad07d"),
    "vibesboxsrc":("commit-8f84376df8b7499808b17c150a328b8665ba1384","8f84376df8b7499808b17c150a328b8665ba1384"),
}
for cid,(version,commit) in expected.items():
    c=components.get(cid)
    if not c:
        raise SystemExit(f"missing component pin: {cid}")
    if c.get("tested_version") != version or c.get("pinned_commit") != commit:
        raise SystemExit(f"unexpected {cid} pin: {c.get('tested_version')} {c.get('pinned_commit')}")

text=pathlib.Path(sys.argv[2]).read_text(encoding="utf-8")
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

echo "AURORA-PI5-EARC-RUNTIME-CONTRACT-PASS"
, b, flags=re.M))}
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
if any("spatialize: false" in b for b in speaker_blocks.values()):
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

echo "AURORA-PI5-EARC-RUNTIME-CONTRACT-PASS"
