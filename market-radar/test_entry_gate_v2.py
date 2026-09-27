from entry_gate_v2 import GateV2Input, entry_gate_v2

def x(**kw):
    d=dict(decision="WATCH",gap_pct=5,premarket_reprice_pct=5,rvol=4,holds_vwap=True,holds_open=True,minutes_since_open=15,spread_pct=.5,above_open_pct=3,above_vwap_pct=2,price=10,gap_pct_abs=5)
    d.update(kw); return GateV2Input(**d)

def test_waits_first_10_minutes(): assert entry_gate_v2(x(minutes_since_open=5))["state"]=="WAIT"
def test_rejects_open_extension(): assert entry_gate_v2(x(above_open_pct=8.1))["state"]=="WAIT"
def test_rejects_vwap_extension(): assert entry_gate_v2(x(above_vwap_pct=6.1))["state"]=="WAIT"
def test_microcap_extreme_gap(): assert entry_gate_v2(x(price=1.5,gap_pct=25,gap_pct_abs=25))["state"]=="WAIT"
def test_clean_setup_buyable(): assert entry_gate_v2(x())["state"]=="BUYABLE_NOW"
def test_inherits_v1_non_buyable(): assert entry_gate_v2(x(rvol=1))["state"]=="WAIT"
