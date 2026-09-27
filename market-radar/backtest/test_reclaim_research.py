import importlib.util
from pathlib import Path
P=Path(__file__).with_name("reclaim_research.py")
s=importlib.util.spec_from_file_location("reclaim",P); m=importlib.util.module_from_spec(s); s.loader.exec_module(m)

def row(t,p,lo,rv=3,ao=3,av=2,ho=True,hv=True):
    return {"time_utc":t,"price":p,"low":lo,"rvol":rv,"above_open_pct":ao,"above_vwap_pct":av,"holds_open":ho,"holds_vwap":hv}

def test_requires_stop_first():
    rows=[row("09:35",100,99),row("09:40",102,100)]
    assert m.detect_reclaim(rows,"09:35",100)["state"]=="NO_RECLAIM"

def test_detects_post_stop_reclaim():
    rows=[row("09:35",100,100),row("09:40",94,94,ho=False,hv=False),row("10:00",101,99)]
    r=m.detect_reclaim(rows,"09:35",100)
    assert r["state"]=="RECLAIM_CANDIDATE" and r["time_utc"]=="10:00"

def test_rejects_chased_reclaim():
    rows=[row("09:35",100,100),row("09:40",94,94,ho=False,hv=False),row("10:00",120,118,ao=12)]
    assert m.detect_reclaim(rows,"09:35",100)["state"]=="NO_RECLAIM"


def test_no_reclaim_exposes_best_candidate_diagnostics():
    rows=[row("09:35",100,100),row("09:40",94,94,ho=False,hv=False),row("10:00",109,107,ao=9,av=4)]
    r=m.detect_reclaim(rows,"09:35",100)
    assert r["state"]=="NO_RECLAIM"
    assert r["best_post_stop_candidate"]["conditions_met"]==4
    assert r["best_post_stop_candidate"]["above_open_pct"]==9
