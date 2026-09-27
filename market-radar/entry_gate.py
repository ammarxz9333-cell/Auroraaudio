#!/usr/bin/env python3
"""Market Radar entry gate.

Converts a catalyst-level WATCH into BUYABLE_NOW only after executable tape
confirmation. This is intentionally separate from catalyst scoring so the
system can discover a good story without recommending a late/bad entry.
"""
from dataclasses import dataclass, asdict

@dataclass
class GateInput:
    decision: str
    gap_pct: float
    premarket_reprice_pct: float
    rvol: float
    holds_vwap: bool
    holds_open: bool
    minutes_since_open: int
    spread_pct: float
    dilution_risk: bool = False
    parabolic_prior_run_pct: float = 0.0

def entry_gate(x: GateInput):
    if x.decision not in {"EARLY","WATCH","WATCH_LOW_CONFIDENCE","WATCH_FLOW","GAP_HOLD"}:
        return {"state":"NO_ENTRY","reason":"base decision is not bullish/watch"}
    if x.spread_pct > 3:
        return {"state":"NO_ENTRY","reason":"spread too wide"}
    if x.premarket_reprice_pct >= 100:
        return {"state":"LATE","reason":"most opportunity may already be consumed premarket"}
    if x.parabolic_prior_run_pct >= 150:
        return {"state":"WAIT","reason":"prior parabolic run raises chase/reversal risk"}
    if x.gap_pct >= 15 and (not x.holds_open or not x.holds_vwap):
        return {"state":"NO_ENTRY","reason":"large gap rejected by tape"}
    if x.rvol < 2:
        return {"state":"WAIT","reason":"insufficient abnormal participation"}
    if x.minutes_since_open < 5 and x.gap_pct >= 10:
        return {"state":"WAIT","reason":"allow large gap to establish acceptance"}
    if x.holds_open and x.holds_vwap and x.rvol >= 2:
        return {"state":"BUYABLE_NOW","reason":"catalyst plus abnormal participation and price acceptance"}
    return {"state":"WAIT","reason":"catalyst valid but entry confirmation incomplete"}

if __name__=="__main__":
    examples={
      "BENF":GateInput("WATCH_LOW_CONFIDENCE",440,262,50,True,True,5,2),
      "WOR":GateInput("WATCH",15,15,5,False,False,15,0.2),
      "GLND":GateInput("WATCH_LOW_CONFIDENCE",10,8,20,True,True,10,1),
      "FLNA":GateInput("WATCH",33,30,20,True,True,10,2)
    }
    for k,v in examples.items(): print(k,entry_gate(v))
