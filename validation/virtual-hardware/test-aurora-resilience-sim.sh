#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [REPORT_PATH]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPORT=${1:-"${TMPDIR:-/tmp}/aurora-resilience-sim.json"}

for cmd in cargo python3; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done

printf '\n== Aurora resilience simulation: real Rust controller/ASRC/FSM evidence ==\n'
cargo run --quiet --release -p aurora-realtime-audio-sim --example resilience_evidence -- "$REPORT"

test -s "$REPORT" || { echo "resilience report missing: $REPORT" >&2; exit 1; }
python3 - "$REPORT" <<'PY'
import json, pathlib, sys

path = pathlib.Path(sys.argv[1])
report = json.loads(path.read_text(encoding="utf-8"))
if report.get("verdict") != "pass":
    raise SystemExit("resilience report verdict is not pass")

clock = report.get("adaptive_clock_rate_correction") or {}
cases = clock.get("cases") or []
if len(cases) != 2:
    raise SystemExit("expected two +/-250 ppm correction cases")
seen = set()
for case in cases:
    ppm = float(case["input_clock_ppm"])
    seen.add(int(ppm))
    estimate = float(case["trusted_estimate_ppm"])
    feedforward = float(case["feedforward_correction_ppm"])
    final = float(case["final_correction_ppm"])
    if abs(estimate - ppm) > 5.0:
        raise SystemExit(f"clock estimate outside tolerance for {ppm}: {estimate}")
    if abs(feedforward + ppm) > 5.0 or abs(final + ppm) > 5.0:
        raise SystemExit(f"clock correction sign/magnitude mismatch for {ppm}")
    if case.get("duration_seconds") != 86400 or case.get("bounded") is not True:
        raise SystemExit(f"24h correction case was not bounded for {ppm}")
    if int(case.get("finite_asrc_sample_blocks", 0)) < 24:
        raise SystemExit(f"ASRC sample path was not periodically exercised for {ppm}")
    if float(case["ring_fill_minimum"]) < 0.0:
        raise SystemExit(f"ring underflow in correction case {ppm}")
    if float(case["ring_fill_maximum"]) >= float(case["ring_capacity_frames"]):
        raise SystemExit(f"ring overflow in correction case {ppm}")
if seen != {-250, 250}:
    raise SystemExit(f"unexpected ppm cases: {seen}")

recovery = report.get("device_reconnect_recovery") or {}
success = recovery.get("successful_reconnect") or {}
exhaustion = recovery.get("budget_exhaustion") or {}
expected_backoff = [250, 500, 1000, 2000, 4000]
if success.get("backoff_ms") != expected_backoff:
    raise SystemExit("successful reconnect did not use bounded exponential backoff")
if success.get("succeeded_on_attempt") != 5 or success.get("final_state") != "Running":
    raise SystemExit("successful reconnect did not recover on the expected bounded attempt")
if success.get("stable_run_reset_attempts") != 0:
    raise SystemExit("stable run did not reset recovery history")
if exhaustion.get("backoff_ms") != expected_backoff:
    raise SystemExit("exhaustion case did not use the same bounded backoff schedule")
if exhaustion.get("attempts") != 5 or exhaustion.get("budget_exhausted") is not True:
    raise SystemExit("reconnect exhaustion did not fail closed at five attempts")
if exhaustion.get("final_state") != "Faulted":
    raise SystemExit("reconnect exhaustion did not remain faulted")

print(
    "AURORA-RESILIENCE-EVIDENCE-PASS "
    "clock=+/-250ppm@24h estimator+feedforward+RubatoAsrc "
    "reconnect=250/500/1000/2000/4000ms attempts=5 fail_closed=true"
)
PY

printf 'report=%s\n' "$REPORT"
