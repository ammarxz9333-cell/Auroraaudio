#!/usr/bin/env python3
"""Export pre-outcome candidate snapshots from Market Radar tape for PIT research."""
from __future__ import annotations
import argparse,datetime as dt,json
from pathlib import Path

def main():
    ap=argparse.ArgumentParser(); ap.add_argument("tape_dir"); ap.add_argument("--out",required=True); a=ap.parse_args()
    rows=[]; seen=set()
    for p in sorted(Path(a.tape_dir).glob("*.jsonl")):
        for line in p.read_text(errors="ignore").splitlines():
            try:r=json.loads(line)
            except Exception:continue
            ticker=r.get("ticker")
            cutoff=r.get("captured_utc")
            bar_time=r.get("bar_time_utc")
            rid=r.get("radar_id")
            if not ticker or not cutoff or not bar_time or not rid:continue
            try:
                observed=dt.datetime.fromisoformat(cutoff.replace("Z","+00:00"))
                bar=dt.datetime.fromisoformat(bar_time.replace("Z","+00:00"))
                if observed.tzinfo is None or bar.tzinfo is None or not dt.timedelta(0) <= observed-bar <= dt.timedelta(minutes=15):
                    continue
            except ValueError:
                continue
            k=(ticker,bar_time,rid)
            if k in seen:continue
            seen.add(k)
            market={x:r.get(x) for x in ("price","change_pct","change_5m_pct","same_time_volume_ratio","market_session","bar_time_utc") if x in r}
            features={"score":r.get("score_at_capture"),"spread_pct":r.get("spread_pct"),"gate_state":r.get("gate_state")}
            rows.append({"ticker":ticker,"cutoff":cutoff,"evidence":[{"id":str(rid),"available_at":cutoff}],"market":market,"features":features})
    Path(a.out).write_text("\n".join(json.dumps(x,separators=(",",":")) for x in rows)+("\n" if rows else ""))
    print(json.dumps({"snapshots":len(rows),"tickers":len({x["ticker"] for x in rows})}))
if __name__=="__main__":main()
