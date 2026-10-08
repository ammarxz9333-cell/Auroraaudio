#!/usr/bin/env python3
"""Point-in-time Market Radar pre-launch evaluator.

Input CSV is deliberately explicit: one row per historical candidate/alert.
The evaluator never infers missing timestamps or prices from future data.
"""
from __future__ import annotations
import argparse, csv, json, statistics
from collections import Counter, defaultdict
from pathlib import Path

FIELDS = [
    "event_date","ticker","family","eventual_move_pct","alert_move_pct",
    "source_to_alert_seconds","false_positive","duplicate","direction_error",
    "tradability_label","confidence"
]

def b(v):
    return str(v).strip().lower() in {"1","true","yes","y"}

def f(v):
    try: return float(v)
    except (TypeError, ValueError): return None

def load(path):
    with open(path, newline="", encoding="utf-8") as h:
        return list(csv.DictReader(h))

def pct(n,d):
    return round(100*n/d,2) if d else None

def evaluate(rows):
    eligible=[r for r in rows if (f(r.get("eventual_move_pct")) or 0)>=10]
    alerted=[r for r in eligible if f(r.get("alert_move_pct")) is not None]
    def before(x): return [r for r in alerted if f(r["alert_move_pct"])<=x]
    lat=[f(r.get("source_to_alert_seconds")) for r in alerted]
    lat=[x for x in lat if x is not None and x>=0]
    moves=[f(r.get("alert_move_pct")) for r in alerted]
    moves=[x for x in moves if x is not None]
    dates={r.get("event_date") for r in rows if r.get("event_date")}
    fp=sum(b(r.get("false_positive")) for r in rows)
    fam=defaultdict(lambda:{"eligible":0,"alerted_before_3pct":0})
    for r in eligible:
        z=fam[r.get("family") or "unknown"]; z["eligible"]+=1
        if f(r.get("alert_move_pct")) is not None and f(r["alert_move_pct"])<=3:
            z["alerted_before_3pct"]+=1
    for z in fam.values():
        z["recall_before_3pct"]=pct(z["alerted_before_3pct"],z["eligible"])
    return {
      "rows":len(rows),"eligible_movers_ge_10pct":len(eligible),
      "recall_before_3pct":pct(len(before(3)),len(eligible)),
      "recall_before_5pct":pct(len(before(5)),len(eligible)),
      "recall_before_8pct":pct(len(before(8)),len(eligible)),
      "median_alert_move_pct":round(statistics.median(moves),3) if moves else None,
      "median_source_to_alert_seconds":round(statistics.median(lat),1) if lat else None,
      "false_positive_alerts":fp,
      "false_positive_alerts_per_session":round(fp/len(dates),3) if dates else None,
      "duplicate_alert_rate_pct":pct(sum(b(r.get("duplicate")) for r in rows),len(rows)),
      "direction_error_rate_pct":pct(sum(b(r.get("direction_error")) for r in rows),len(rows)),
      "tradability_labels":dict(Counter(r.get("tradability_label") or "UNKNOWN" for r in rows)),
      "families":dict(fam),
      "gate_note":"Do not promote rules when recall rises only by increasing false positives, duplicates, or direction errors."
    }

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("csv")
    ap.add_argument("--out")
    a=ap.parse_args()
    result=evaluate(load(a.csv))
    payload=json.dumps(result,indent=2,sort_keys=True)
    print(payload)
    if a.out: Path(a.out).write_text(payload+"\n",encoding="utf-8")
if __name__=="__main__": main()
