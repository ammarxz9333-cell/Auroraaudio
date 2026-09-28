from entry_gate_v3 import GateV3Input, entry_gate_v3

def x(decision="BEARISH_AVOID",**kw):
    d=dict(decision=decision,gap_pct=3,premarket_reprice_pct=3,rvol=4,holds_vwap=True,holds_open=True,minutes_since_open=15,spread_pct=.5,above_open_pct=3,above_vwap_pct=2,price=5,gap_pct_abs=3,catalyst_decision=decision)
    d.update(kw); return GateV3Input(**d)

def test_risky_can_be_tape_confirmed(): assert entry_gate_v3(x())["state"]=="BUYABLE_NOW"
def test_requires_acceptance(): assert entry_gate_v3(x(holds_vwap=False))["state"]=="WAIT"
def test_rejects_chase(): assert entry_gate_v3(x(above_open_pct=9))["state"]=="WAIT"
def test_microcap_extreme_stays_wait(): assert entry_gate_v3(x(price=1.2,gap_pct=25,gap_pct_abs=25))["state"]=="WAIT"
def test_normal_delegates_v2(): assert entry_gate_v3(x("WATCH"))["state"]=="BUYABLE_NOW"
