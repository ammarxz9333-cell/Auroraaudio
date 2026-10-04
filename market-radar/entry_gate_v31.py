#!/usr/bin/env python3
"""Gate v3.1 research-only: allow open extension only under strong contemporaneous tape acceptance."""
from dataclasses import dataclass
from entry_gate_v3 import GateV3Input, entry_gate_v3, RISK_DECISIONS

@dataclass
class GateV31Input(GateV3Input):
    bar_close_location: float = 0.5

def entry_gate_v31(x: GateV31Input):
    d=x.catalyst_decision or x.decision
    if d not in RISK_DECISIONS:
        return entry_gate_v3(x)
    if x.price and x.price < 2 and (x.gap_pct_abs or abs(x.gap_pct)) >= 20:
        return {"state":"WAIT","reason":"v3.1 extreme microcap risk regime"}
    if x.minutes_since_open < 10:
        return {"state":"WAIT","reason":"v3.1 risky catalyst requires >=10m tape acceptance"}
    if x.rvol < 2:
        return {"state":"WAIT","reason":"v3.1 risky catalyst lacks abnormal participation"}
    if not x.holds_open or not x.holds_vwap:
        return {"state":"WAIT","reason":"v3.1 risky catalyst lacks open/VWAP acceptance"}
    if x.above_vwap_pct > 6:
        return {"state":"WAIT","reason":"v3.1 risky catalyst too extended above VWAP"}
    if x.above_open_pct > 8:
        strong_extension_acceptance = x.rvol >= 4 and x.bar_close_location >= 0.80
        if not strong_extension_acceptance:
            return {"state":"WAIT","reason":"v3.1 open extension lacks strong tape confirmation"}
    return {"state":"BUYABLE_NOW","reason":"v3.1 risk acknowledged with strong tape acceptance"}
