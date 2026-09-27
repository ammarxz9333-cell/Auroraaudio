#!/usr/bin/env python3
"""Counterfactual outcome evaluator for rejected post-stop recovery candidates."""
def evaluate_candidate(rows, candidate):
    if not candidate:
        return None
    ep=float(candidate["price"])
    start=next((i for i,r in enumerate(rows) if r["time_utc"]==candidate["time_utc"]),None)
    if start is None:
        return None
    outcome=None
    outcome_time=None
    plus10=False
    mfe=0.0
    mae=0.0
    for r in rows[start:]:
        hi=float(r["high"])
        lo=float(r["low"])
        mfe=max(mfe,(hi/ep-1)*100)
        mae=min(mae,(lo/ep-1)*100)
        plus10=plus10 or hi>=ep*1.10
        if outcome is None:
            p5=hi>=ep*1.05
            m5=lo<=ep*0.95
            outcome=("ORDER_UNVERIFIED" if p5 and m5 else "PLUS5_FIRST" if p5 else "MINUS5_FIRST" if m5 else None)
            if outcome:
                outcome_time=r["time_utc"]
    return {"outcome":outcome or "UNRESOLVED","outcome_time":outcome_time,"plus10_reached":plus10,"mfe_pct":round(mfe,3),"mae_pct":round(mae,3)}
