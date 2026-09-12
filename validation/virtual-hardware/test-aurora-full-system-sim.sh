#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_DIR]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MOVING_TEST="$ROOT_DIR/validation/immersive/test-joc-aurora-moving.sh"
SIMULATOR="$ROOT_DIR/validation/virtual-hardware/aurora_full_system_sim.py"
COVERAGE="$ROOT_DIR/validation/virtual-hardware/validate_simulation_coverage.py"
OUTPUT_DIR=${1:-"$(mktemp -d "${TMPDIR:-/tmp}/aurora-full-system-sim.XXXXXX")"}
MOVING_DIR="$OUTPUT_DIR/moving-joc"
SIM_DIR="$OUTPUT_DIR/virtual-hardware"
DSP_RENDER="$SIM_DIR/aurora-output-dsp-7.1.4.f32"
DSP_LOG="$SIM_DIR/aurora-output-dsp.log"

for cmd in python3 bash cargo; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done
for path in "$MOVING_TEST" "$SIMULATOR" "$COVERAGE"; do
  [[ -f "$path" ]] || { echo "missing validation dependency: $path" >&2; exit 2; }
done
mkdir -p "$SIM_DIR"

printf '\n== Aurora full-system simulation: coverage contract ==\n'
python3 "$COVERAGE" self-test
python3 "$COVERAGE" check

printf '\n== Aurora full-system simulation: analyzer self-test ==\n'
python3 "$SIMULATOR" self-test

printf '\n== Aurora full-system simulation: reproduce proven moving-JOC path ==\n'
bash "$MOVING_TEST" "$MOVING_DIR"

RENDER="$MOVING_DIR/aurora-moving/aurora-moving-paced-7.1.4.f32"
JOC_EVIDENCE="$MOVING_DIR/aurora-moving/aurora-joc-moving-evidence.json"
[[ -s "$RENDER" ]] || { echo "missing paced 7.1.4 render: $RENDER" >&2; exit 1; }
[[ -s "$JOC_EVIDENCE" ]] || { echo "missing moving-JOC evidence: $JOC_EVIDENCE" >&2; exit 1; }

printf '\n== Aurora full-system simulation: real Aurora output DSP ==\n'
cargo run --quiet --release -p aurora-dsp-basic --example process_7_1_4_file -- \
  "$RENDER" "$DSP_RENDER" 0 | tee "$DSP_LOG"
[[ -s "$DSP_RENDER" ]] || { echo "Aurora output DSP produced no render" >&2; exit 1; }
python3 - "$RENDER" "$DSP_RENDER" <<'PY'
import hashlib, pathlib, sys
source = pathlib.Path(sys.argv[1])
processed = pathlib.Path(sys.argv[2])
if source.stat().st_size != processed.stat().st_size:
    raise SystemExit("output DSP changed frame count or channel shape")
def sha(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()
source_sha = sha(source)
processed_sha = sha(processed)
if source_sha == processed_sha:
    raise SystemExit("output DSP unexpectedly behaved as a byte-identical passthrough")
print(f"AURORA-OUTPUT-DSP-INTEGRATION-PASS bytes={processed.stat().st_size} source_sha={source_sha} processed_sha={processed_sha}")
PY

printf '\n== Aurora full-system simulation: healthy virtual TDM16/DAC path ==\n'
HEALTHY_REPORT="$SIM_DIR/aurora-full-system-sim.json"
python3 "$SIMULATOR" run \
  --render "$DSP_RENDER" \
  --joc-evidence "$JOC_EVIDENCE" \
  --report "$HEALTHY_REPORT" \
  --fault none \
  --tdm-slots 16 \
  --latency-frames 256 \
  --max-latency-frames 1024

printf '\n== Aurora full-system simulation: fail-closed fault profiles ==\n'
for fault in dropout channel-silence channel-swap disconnect drift sample-rate-change latency-spike non-finite tdm-padding-corruption; do
  report="$SIM_DIR/fault-${fault}.json"
  set +e
  python3 "$SIMULATOR" run \
    --render "$DSP_RENDER" \
    --joc-evidence "$JOC_EVIDENCE" \
    --report "$report" \
    --fault "$fault" \
    --tdm-slots 16 \
    --latency-frames 256 \
    --max-latency-frames 1024
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
if not health.get("order_preserved") or not health.get("pcm_identity_preserved"):
    raise SystemExit("healthy profile did not preserve channel/PCM identity across virtual transport")
if hw["xrun_count"] != 0 or hw["disconnect_seen"]:
    raise SystemExit("healthy profile contains virtual output failure")
if hw["sink_sample_rate_hz"] != 48000:
    raise SystemExit("healthy virtual sink sample rate changed")
if hw["tdm_slots"] != 16 or hw["unused_zero_slots"] != [12, 13, 14, 15] or hw["tdm_padding_nonzero_samples"] != 0:
    raise SystemExit("unexpected TDM16 slot mapping/padding")
if hw["simulated_latency_frames"] > hw["simulated_latency_budget_frames"]:
    raise SystemExit("healthy virtual path exceeds latency budget")
print(
    "AURORA-FULL-SYSTEM-SIM-PROOF-PASS "
    f"frames={hw['sink_frames']} channels={len(health['active_channel_indices'])} "
    f"tdm_slots={hw['tdm_slots']} xruns={hw['xrun_count']} "
    f"latency_ms_simulated={hw['simulated_latency_ms']:.3f}"
)
PY

printf 'evidence_dir=%s\n' "$OUTPUT_DIR"
