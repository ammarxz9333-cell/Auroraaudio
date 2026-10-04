import importlib.util
from pathlib import Path

p=Path(__file__).with_name("rejected_recovery_outcome.py")
s=importlib.util.spec_from_file_location("rejected",p)
m=importlib.util.module_from_spec(s)
s.loader.exec_module(m)

def test_plus5_first():
    rows=[{"time_utc":"t0","high":101,"low":99},{"time_utc":"t1","high":106,"low":98}]
    r=m.evaluate_candidate(rows,{"time_utc":"t0","price":100})
    assert r["outcome"]=="PLUS5_FIRST"
    assert r["outcome_time"]=="t1"

def test_minus5_first_but_later_plus10():
    rows=[{"time_utc":"t0","high":101,"low":94},{"time_utc":"t1","high":101,"low":94},{"time_utc":"t2","high":111,"low":96}]
    r=m.evaluate_candidate(rows,{"time_utc":"t0","price":100})
    assert r["outcome"]=="MINUS5_FIRST"
    assert r["plus10_reached"] is True
    assert r["mfe_pct"]==11.0
    assert r["mae_pct"]==-6.0

def test_same_bar_is_unverified():
    rows=[{"time_utc":"t0","high":106,"low":94},{"time_utc":"t1","high":106,"low":94}]
    r=m.evaluate_candidate(rows,{"time_utc":"t0","price":100})
    assert r["outcome"]=="ORDER_UNVERIFIED"

def test_signal_bar_extreme_is_not_a_trade_result():
    rows=[{"time_utc":"t0","high":120,"low":80},{"time_utc":"t1","high":101,"low":99}]
    r=m.evaluate_candidate(rows,{"time_utc":"t0","price":100})
    assert r["outcome"]=="UNRESOLVED"
