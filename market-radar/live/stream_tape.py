#!/usr/bin/env python3
"""Provider-neutral streaming tape analytics for HOT WATCHLIST symbols."""
from collections import defaultdict, deque
from datetime import datetime, timedelta, timezone

def parse_time(value):
    try:
        return datetime.fromisoformat(str(value).replace("Z","+00:00")).astimezone(timezone.utc)
    except Exception:
        return None

class Tape:
    def __init__(self):
        self.rows=defaultdict(lambda:deque(maxlen=20000))

    def add(self,event):
        ticker=str(event.get("ticker","")).upper()
        when=parse_time(event.get("ts"))
        price=float(event.get("price",0) or 0)
        size=float(event.get("size",0) or 0)
        if not ticker or when is None or price <= 0:
            return None
        q=self.rows[ticker]
        q.append((when,price,size))
        cutoff=when-timedelta(minutes=35)
        while q and q[0][0] < cutoff:
            q.popleft()
        return self.signal(ticker,event)

    def signal(self,ticker,event):
        q=self.rows[ticker]
        now,price,_=q[-1]
        def move(minutes):
            target=now-timedelta(minutes=minutes)
            old=next((x for x in q if x[0] >= target),None)
            return round((price/old[1]-1)*100,3) if old and old[1] else None
        m5,m15,m30=move(5),move(15),move(30)
        previous=float(event.get("prev_close",0) or 0)
        day=(price/previous-1)*100 if previous else None
        rvol=event.get("same_time_rvol")
        rvol=float(rvol) if rvol is not None else None
        acceleration=max(abs(x or 0) for x in (m5,m15,m30))
        early=day is not None and 0.5 <= day <= 3.5 and acceleration >= 0.6 and rvol is not None and rvol >= 2
        return {
            "ticker":ticker,"ts":now.isoformat(),"price":price,
            "day_pct":round(day,3) if day is not None else None,
            "m5":m5,"m15":m15,"m30":m30,"same_time_rvol":rvol,
            "classification":"EARLY_HEADS_UP" if early else "WATCH"
        }
