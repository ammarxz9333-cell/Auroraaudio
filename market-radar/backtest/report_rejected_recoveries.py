#!/usr/bin/env python3
import json,sys
from pathlib import Path
from rejected_recovery_outcome import evaluate_candidate

def main():
    replay=json.loads(Path(sys.argv[1]).read_text())["cases"]
    rows=[]
    for case in replay:
        rec=case.get("reclaim_research") or {}
        cand=rec.get("best_post_stop_candidate")
        cache=case.get("bars_cache")
        if not cand or not cache or not Path(cache).exists():
            continue
        raw=json.loads(Path(cache).read_text())
        regular=[]
        for b in raw:
            ny=b["ny"]
            hhmm=ny[11:16]
            if "T" in ny and "09:30" <= hhmm < "16:00":
                regular.append({"time_utc":b["time"],"high":b["high"],"low":b["low"]})
        result=evaluate_candidate(regular,cand)
        rows.append({"ticker":case["ticker"],"recovery_type":cand.get("recovery_type"),"blockers":cand.get("blockers"),"candidate_time":cand["time_utc"],"entry_price":cand["price"],"counterfactual":result})
    print(json.dumps({"rejected_recovery_counterfactuals":rows},indent=2))

if __name__=="__main__":
    main()
