#!/usr/bin/env python3
"""Export pre-outcome candidate snapshots from Market Radar tape for PIT research."""
from __future__ import annotations
import argparse,json
from pathlib import Path

def main():
    ap=argparse.ArgumentParser(); ap.add_argument("tape_dir"); ap.add_argument("--out",required=True); a=ap.parse_args()
    rows=[]; seen=set()
    for p in sorted(Path(a.tape_dir).glob("*.jsonl")):
        for line in p.read_text(errors="ignore").splitlines():
            try:r=json.loads(line)
            except Exception:continue
            ticker=r.get("ticker"); cutoff=r.get("bar_time_utc") or r.get("timestamp_utc")
            if not ticker or not cutoff:continue
            k=(ticker,cutoff,r.get("source_item_id"))
            if k in seen:continue
            seen.add(k)
            market={x:r.get(x) for x in ("price","change_pct","change_5m_pct","same_time_volume_ratio","pre30m_move_pct","post30m_move_pct","since_event_move_pct","market_session") if x in r}
            features={x:r.get(x) for x in ("score","threshold","gap_pct","premarket_reprice_pct","rvol","holds_vwap","holds_open","spread_pct") if x in r}
            rows.append({"ticker":ticker,"cutoff":cutoff,"evidence":[{"id":str(r.get("source_item_id") or p.name),"available_at":str(r.get("event_time_utc") or cutoff)}],"market":market,"features":features})
    Path(a.out).write_text("\n".join(json.dumps(x,separators=(",",":")) for x in rows)+("\n" if rows else ""))
    print(json.dumps({"snapshots":len(rows),"tickers":len({x["ticker"] for x in rows})}))
if __name__=="__main__":main()
