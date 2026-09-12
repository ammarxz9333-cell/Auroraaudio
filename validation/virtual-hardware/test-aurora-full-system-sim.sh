#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MOVING_TEST="$ROOT_DIR/validation/immersive/test-joc-aurora-moving.sh"
SIMULATOR="$ROOT_DIR/validation/virtual-hardware/aurora_full_system_sim.py"
OUTPUT_DIR=${1:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-full-system-sim.XXXXXX")"}
MOVING_DIR="$OUTPUT_DIR/moving-joc"
SIM_DIR="$OUTPUT_DIR/virtual-hardware"

for cmd in python3 bash; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
for path in "$MOVING_TEST" "$SIMULATOR"; do
  [[ -f "$path" ]] || { echo "missing validation dependency: $path" >&2; exit 2; }
done
mkdir -p "$SIM_DIR"

printf '\n== Aurora full-system simulation: analyzer self-test ==\n'
python3 "$SIMULATOR" self-test

printf '\n== Aurora full-system simulation: reproduce proven moving-JOC path ==\n'
bash "$MOVING_TEST" "$MOVING_DIR"

RENDER="$MOVING_DIR/aurora-moving/aurora-moving-paced-7.1.4.f32"
JOC_EVIDENCE="$MOVING_DIR/aurora-moving/aurora-joc-moving-evidence.json"
[[ -s "$RENDER" ]] || { echo "missing paced 7.1.4 render: $RENDER" >&2; exit 1; }
[[ -s "$JOC_EVIDENCE" ]] || { echo "missing moving-JOC evidence: $JOC_EVIDENCE" >&2; exit 1; }

printf '\n== Aurora full-system simulation: healthy virtual TDM16/DAC path ==\n'
HEALTHY_REPORT="$SIM_DIR/aurora-full-system-sim.json"
python3 "$SIMULATOR" run \
  --render "$RENDER" \
  --joc-evidence "$JOC_EVIDENCE" \
  --report "$HEALTHY_REPORT" \
  --fault none \
  --tdm-slots 16 \
  --latency-frames 256

printf '\n== Aurora full-system simulation: fail-closed fault profiles ==\n'
for fault in dropout channel-silence disconnect drift; do
  report="$SIM_DIR/fault-${fault}.json"
  set +e
  python3 "$SIMULATOR" run \
    --render "$RENDER" \
    --joc-evidence "$JOC_EVIDENCE" \
    --report "$report" \
    --fault "$fault" \
    --tdm-slots 16 \
    --latency-frames 256
  status=$?
  set -e
  if [[ $status -eq 0 ]]; then
    echo "fault profile unexpectedly passed: $fault" >&2
    exit 1
  fi
  if [[ $status -ne 1 ]]; then
    echo "fault profile failed with infrastructure error instead of evidence failure: $fault status=$status" >&2
    exit 1
  fi
  python3 - "$report" "$fault" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
fault = sys.argv[2]
report = json.loads(path.read_text(encoding="utf-8"))
if report.get("verdict") != "fail":
    raise SystemExit(f"negative profile did not emit fail verdict: {fault}")
if not report.get("failures"):
    raise SystemExit(f"negative profile emitted no failure reasons: {fault}")
print(f"AURORA-FULL-SYSTEM-SIM-NEGATIVE-PASS fault={fault} failures={len(report['failures'])}")
PY
done

python3 - "$HEALTHY_REPORT" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
report = json.loads(path.read_text(encoding="utf-8"))
if report.get("verdict") != "pass":
    raise SystemExit("healthy virtual hardware report is not pass")
source = report["source"]
hw = report["virtual_hardware"]
health = report["channel_health"]
if source["expected_frames"] != source["actual_frames"] or source["expected_frames"] != hw["sink_frames"]:
    raise SystemExit("healthy frame accounting mismatch")
if len(health["active_channel_indices"]) != 12:
    raise SystemExit("healthy profile does not have all 12 output channels active")
if hw["xrun_count"] != 0 or hw["disconnect_seen"]:
    raise SystemExit("healthy profile contains virtual output failure")
if hw["tdm_slots"] != 16 or hw["unused_zero_slots"] != [12, 13, 14, 15]:
    raise SystemExit("unexpected TDM16 slot mapping")
print(
    "AURORA-FULL-SYSTEM-SIM-PROOF-PASS "
    f"frames={hw['sink_frames']} channels={len(health['active_channel_indices'])} "
    f"tdm_slots={hw['tdm_slots']} xruns={hw['xrun_count']} "
    f"latency_ms_simulated={hw['simulated_latency_ms']:.3f}"
)
PY

printf 'evidence_dir=%s\n' "$OUTPUT_DIR"
