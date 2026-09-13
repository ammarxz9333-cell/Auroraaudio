#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [REPORT_PATH]" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPORT=${1:-"${TMPDIR:-/tmp}/aurora-resilience-sim.json"}
TRANSITION_REPORT="${REPORT}.transitions.json"

for cmd in cargo python3; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "missing required command: $cmd" >&2; exit 2; }
done

printf '\n== Aurora resilience simulation: real Rust controller/ASRC/FSM evidence ==\n'
cargo run --quiet --release -p aurora-realtime-audio-sim --example resilience_evidence -- "$REPORT"
cargo run --quiet --release -p aurora-realtime-audio-sim --example resilience_transition_evidence -- "$TRANSITION_REPORT"

test -s "$REPORT" || { echo "resilience report missing: $REPORT" >&2; exit 1; }
test -s "$TRANSITION_REPORT" || { echo "resilience transition report missing: $TRANSITION_REPORT" >&2; exit 1; }
python3 - "$REPORT" "$TRANSITION_REPORT" <<'PY'
import json, pathlib, sys

path = pathlib.Path(sys.argv[1])
transition_path = pathlib.Path(sys.argv[2])
report = json.loads(path.read_text(encoding="utf-8"))
transitions = json.loads(transition_path.read_text(encoding="utf-8"))
if report.get("verdict") != "pass":
    raise SystemExit("resilience report verdict is not pass")
if transitions.get("verdict") != "pass":
    raise SystemExit("resilience transition report verdict is not pass")

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

epoch = clock.get("discontinuity_reacquire") or {}
if epoch.get("feedforward_cleared_on_discontinuity") is not True or epoch.get("passed") is not True:
    raise SystemExit("clock discontinuity did not clear trust/feed-forward")
if abs(float(epoch.get("initial_estimate_ppm", 0.0)) - 250.0) > 5.0:
    raise SystemExit("initial clock epoch estimate is outside tolerance")
if abs(float(epoch.get("reacquired_estimate_ppm", 0.0)) + 250.0) > 5.0:
    raise SystemExit("clock estimator did not reacquire the new epoch")
if abs(float(epoch.get("reacquired_feedforward_ppm", 0.0)) - 250.0) > 5.0:
    raise SystemExit("reacquired feed-forward has wrong sign/magnitude")

out_of_range = clock.get("out_of_range_fail_closed") or {}
if out_of_range.get("rejected") is not True:
    raise SystemExit("out-of-range clock relationship was not rejected")
if float(out_of_range.get("feedforward_after_rejection_ppm", 1.0)) != 0.0:
    raise SystemExit("out-of-range clock rejection left stale feed-forward active")

hard_latch = clock.get("adaptive_fault_latch") or {}
if hard_latch.get("passed") is not True or hard_latch.get("fault_latched") is not True:
    raise SystemExit("unsupported adaptive fault did not enter a persistent mute latch")
if hard_latch.get("fault") != "Controller" or hard_latch.get("health") != "Fatal":
    raise SystemExit("adaptive hard-fault latch did not publish Controller/Fatal status")
if hard_latch.get("first_callback_muted") is not True or hard_latch.get("second_callback_muted") is not True:
    raise SystemExit("adaptive hard-fault latch leaked non-silent output")
if int(hard_latch.get("underflow_count", -1)) != 0:
    raise SystemExit("latched adaptive hard fault was miscounted as a normal underflow")
if hard_latch.get("clock_epoch_unchanged_after_latched_callback") is not True:
    raise SystemExit("latched adaptive hard fault mutated its clock epoch without recovery")
if hard_latch.get("requires_control_plane_bridge_rebuild") is not True:
    raise SystemExit("adaptive hard fault did not preserve explicit control-plane recovery policy")

jitter = transitions.get("clock_jitter_median_filter") or {}
if jitter.get("passed") is not True:
    raise SystemExit("clock jitter profile did not pass")
if abs(float(jitter.get("trusted_filtered_ppm", 0.0)) - 250.0) > 5.0:
    raise SystemExit("clock jitter median filter did not converge to the expected estimate")
if abs(float(jitter.get("feedforward_correction_ppm", 0.0)) + 250.0) > 5.0:
    raise SystemExit("clock jitter profile produced the wrong feed-forward correction")
if abs(float(jitter.get("first_applied_correction_ppm", 999.0))) > 2.000001:
    raise SystemExit("clock jitter feed-forward bypassed slew limiting")

step = transitions.get("clock_step_bounded_slew") or {}
if step.get("passed") is not True or step.get("asrc_ratio_path_exercised") is not True:
    raise SystemExit("continuous clock-step profile did not exercise the ASRC ratio path")
if abs(float(step.get("stepped_feedforward_correction_ppm", 0.0)) - 250.0) > 5.0:
    raise SystemExit("clock step was not re-estimated with the expected sign")
if abs(float(step.get("final_correction_ppm", 0.0)) - 250.0) > 5.0:
    raise SystemExit("clock step correction did not converge to the new feed-forward target")
if float(step.get("maximum_observed_step_ppm", 999.0)) > 2.000001:
    raise SystemExit("clock step violated the configured correction slew limit")
updates = int(step.get("updates_to_converge", 0))
if not 1 <= updates <= 300:
    raise SystemExit("clock step convergence escaped the bounded update horizon")

recovery = report.get("device_reconnect_recovery") or {}
success = recovery.get("successful_reconnect") or {}
exhaustion = recovery.get("budget_exhaustion") or {}
flapping = recovery.get("flapping_device") or {}
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
if flapping.get("backoff_ms") != expected_backoff:
    raise SystemExit("flapping reconnect did not preserve exponential backoff history")
if flapping.get("successful_reopens_without_stable_run") != 5:
    raise SystemExit("flapping profile did not exercise five successful unstable reopens")
if flapping.get("attempts_retained") != 5 or flapping.get("sixth_fault_recovery_permitted") is not False:
    raise SystemExit("flapping device incorrectly reset or exceeded recovery budget")
if flapping.get("final_state") != "Faulted":
    raise SystemExit("flapping device did not terminate fail-closed")

print(
    "AURORA-RESILIENCE-EVIDENCE-PASS "
    "clock=+/-250ppm@24h+jitter+step+discontinuity+out_of_range+hard_latch estimator+feedforward+RubatoAsrc "
    "reconnect=bounded_success+exhaustion+flapping fail_closed=true"
)
PY

printf 'report=%s\ntransition_report=%s\n' "$REPORT" "$TRANSITION_REPORT"
