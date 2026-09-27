import datetime as dt
import research_market as r

def bar(minute,close,high=None):
    t=dt.datetime(2026,1,2,14,30,tzinfo=dt.timezone.utc)+dt.timedelta(minutes=minute)
    ny=t.astimezone(dt.timezone(dt.timedelta(hours=-5)))
    return {"utc":t,"ny":ny,"close":close,"high":high or close}

def test_relative_strength():
    x=r.relative_strength({"price":110,"previous_close":100},{"price":101,"previous_close":100},{"price":102,"previous_close":100})
    assert x["vs_spy_pct"]==9.0 and x["vs_qqq_pct"]==8.0

def test_halt_gap():
    bars=[bar(0,10),bar(5,10.1),bar(30,10.5)]
    x=r.possible_halt(bars)
    assert x["possible_halt"] is True and x["gap_minutes"]==25.0

def test_timeframes():
    bars=[bar(i*5,100+i) for i in range(13)]
    x=r.timeframe_features(bars)
    assert x["return_60m_pct"]==12.0
