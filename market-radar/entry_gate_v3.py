#!/usr/bin/env python3
"""Gate v3 research: separate catalyst risk from tape opportunity. Never used by live gate."""
from dataclasses import dataclass
from entry_gate_v2 import GateV2Input, entry_gate_v2

RISK_DECISIONS={"BEARISH_AVOID","WATCH_LOW_CONFIDENCE","WATCH_FINANCING"}

@dataclass
class GateV3Input(GateV2Input):
    catalyst_decision: str = ""

def entry_gate_v3(x: GateV3Input):
    d=x.catalyst_decision or x.decision
    if d not in RISK_DECISIONS:
        return entry_gate_v2(x)
    if x.price and x.price < 2 and (x.gap_pct_abs or abs(x.gap_pct)) >= 20:
        return {"state":"WAIT","reason":"v3 extreme microcap risk regime"}
    if x.minutes_since_open < 10:
        return {"state":"WAIT","reason":"v3 risky catalyst requires >=10m tape acceptance"}
    if x.rvol < 2:
        return {"state":"WAIT","reason":"v3 risky catalyst lacks abnormal participation"}
    if not x.holds_open or not x.holds_vwap:
        return {"state":"WAIT","reason":"v3 risky catalyst lacks open/VWAP acceptance"}
    if x.above_open_pct > 8 or x.above_vwap_pct > 6:
        return {"state":"WAIT","reason":"v3 risky catalyst too extended"}
    return {"state":"BUYABLE_NOW","reason":"v3 tape override: risk acknowledged, momentum independently confirmed"}
