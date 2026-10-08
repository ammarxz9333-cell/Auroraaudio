#!/usr/bin/env python3
"""Dynamic HOT WATCHLIST state.

Catalyst layer can seed a symbol for a short TTL; a streaming market-data
adapter can then subscribe only to these names. JSON+atomic replace keeps the
interface provider-neutral and safe across daemon restarts.
"""
from __future__ import annotations
import json, os, tempfile
from datetime import datetime, timedelta, timezone
from pathlib import Path

DEFAULT=Path(__file__).with_name("hot_watchlist.json")

def _now(): return datetime.now(timezone.utc)
def _parse(s):
    try: return datetime.fromisoformat(str(s).replace("Z","+00:00"))
    except Exception: return None

def load(path=DEFAULT, now=None):
    p=Path(path); now=now or _now()
    if not p.exists(): return {}
    try: data=json.loads(p.read_text(encoding="utf-8"))
    except Exception: return {}
    return {k:v for k,v in data.items() if (_parse(v.get("expires_at")) or now) > now}

def save(items,path=DEFAULT):
    p=Path(path); p.parent.mkdir(parents=True,exist_ok=True)
    fd,tmp=tempfile.mkstemp(prefix=p.name+".",dir=str(p.parent))
    try:
        with os.fdopen(fd,"w",encoding="utf-8") as h: json.dump(items,h,indent=2,sort_keys=True); h.write("\n")
        os.replace(tmp,p)
    finally:
        if os.path.exists(tmp): os.unlink(tmp)

def seed(ticker, reason, family="unknown", source_time=None, score=None, ttl_minutes=180, path=DEFAULT, now=None):
    now=now or _now(); items=load(path,now)
    t=str(ticker).upper().strip()
    items[t]={"ticker":t,"reason":reason,"family":family,"source_time":source_time,
              "score":score,"seeded_at":now.isoformat(),
              "expires_at":(now+timedelta(minutes=ttl_minutes)).isoformat()}
    save(items,path); return items[t]

def symbols(path=DEFAULT,now=None): return sorted(load(path,now))
