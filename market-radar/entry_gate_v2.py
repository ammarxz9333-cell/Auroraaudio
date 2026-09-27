#!/usr/bin/env python3
"""Entry Gate v2 research candidate. V1 remains frozen and unchanged."""
from dataclasses import dataclass
from entry_gate import GateInput, entry_gate

@dataclass
class GateV2Input(GateInput):
    above_open_pct: float = 0.0
    above_vwap_pct: float = 0.0
    price: float = 0.0
    gap_pct_abs: float = 0.0

def entry_gate_v2(x: GateV2Input):
    base=entry_gate(x)
    if base["state"]!="BUYABLE_NOW":
        return base
    # Experimental risk regime: penny/very-low-price names with extreme gaps need separate handling.
    if x.price and x.price < 2 and (x.gap_pct_abs or abs(x.gap_pct)) >= 20:
        return {"state":"WAIT","reason":"v2.1 extreme microcap regime requires separate confirmation"}
    if x.minutes_since_open < 10:
        return {"state":"WAIT","reason":"v2 requires at least 10 minutes of regular-session acceptance"}
    if x.above_open_pct > 8:
        return {"state":"WAIT","reason":"v2 anti-chase: price extended >8% above regular open"}
    if x.above_vwap_pct > 6:
        return {"state":"WAIT","reason":"v2 anti-chase: price extended >6% above VWAP"}
    return {"state":"BUYABLE_NOW","reason":"v2 confirmed acceptance without excessive extension"}
