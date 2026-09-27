#!/usr/bin/env python3
"""Self-test leakage guard and decision lock."""
import importlib.util, json, tempfile
from pathlib import Path
HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location("pit",HERE/"pit_backtester.py")
pit=importlib.util.module_from_spec(spec);spec.loader.exec_module(pit)
snap={"ticker":"TEST","cutoff":"2026-09-18T16:00:00-04:00","evidence":[{"id":"x","available_at":"2026-09-18T15:59:00-04:00"}],"market":{"price":10},"features":{},"market_regime":{"SPY_return_at_cutoff":0.2}}
r=pit.freeze(snap,"WATCH","test")
assert pit.verify(r)
tampered=dict(r);tampered["decision"]="EARLY"
assert not pit.verify(tampered)
try:
    bad=dict(snap);bad["future_return_pct"]=20;pit.freeze(bad,"WATCH","bad")
    raise AssertionError("leakage accepted")
except ValueError: pass
try:
    late=json.loads(json.dumps(snap));late["evidence"][0]["available_at"]="2026-09-18T16:01:00-04:00";pit.freeze(late,"WATCH","bad")
    raise AssertionError("post-cutoff evidence accepted")
except ValueError: pass
print("PASS")
