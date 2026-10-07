#!/usr/bin/env python3
"""Council I/O bridge: one contract for UI, ChatGPT and automation clients."""
import argparse, datetime as dt, json
from pathlib import Path
import importlib.util

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location("council_memory",HERE/"council_memory.py")
cm=importlib.util.module_from_spec(spec); spec.loader.exec_module(cm)

FRESHNESS={
 "price_chart":"intraday/current session","financials":"latest reported quarter plus any newer preliminary results",
 "analyst_targets":"latest available ratings/targets","earnings_estimates":"latest estimates/revisions and post-release actuals",
 "insider_trades":"latest 6 months","congress_trades":"latest 12 months","news":"latest material catalyst",
 "sec_filings":"latest material filing plus current 10-Q/10-K","institutional_holdings":"latest available 13F",
 "options_market":"current/latest liquid chain","economy":"current macro regime","related_markets":"current sector/peer tape"
}

def preflight(ticker=None):
    c=cm.context()
    return {"ticker":ticker,"required_agents":list(cm.AGENTS),"horizons":list(cm.HORIZONS),
      "learning":c,"freshness_policy":FRESHNESS,
      "instructions":[
       "Run all 12 agents independently from one point-in-time snapshot.",
       "Every seat must include evidence, evidence_as_of, uncertainty and invalidation. Use NO_READ/neutral when evidence is unavailable.",
       "Newer material company disclosures override stale trailing metrics for current-growth claims; retain both and label the periods.",
       "The Challenger must flag stale-vs-fresh conflicts, contradictory period definitions, duplicated narratives, implausible targets and illiquid-options overconfidence.",
       "Final Judge must resolve flagged conflicts before aggregation and use learned weights only as reliability evidence.",
       "Keep P(up), expected return and forecast range separate.",
       "The displayed UI run and durable prediction must be the same prediction object/ID; do not recompute a second council for persistence.",
       "After Final Judge, persist the complete prediction and verify the ID exists in the durable ledger."
      ]}

def audit(x):
    issues=[]; agents=x.get("agents") or {}
    for a in cm.AGENTS:
        s=agents.get(a) or {}
        if "evidence_as_of" not in s: issues.append({"kind":"missing_freshness","agent":a})
        ev=str(s.get("evidence",""))
        if not ev: issues.append({"kind":"missing_evidence","agent":a})
    if "challenger" not in x: issues.append({"kind":"missing_challenger"})
    if "final" not in x: issues.append({"kind":"missing_final"})
    return issues

def record(x):
    x=dict(x); x.setdefault("contract_version","2.0")
    x.setdefault("audit",audit(x))
    return cm.record(x)

def main():
    ap=argparse.ArgumentParser(); sp=ap.add_subparsers(dest="cmd",required=True)
    p=sp.add_parser("preflight"); p.add_argument("--ticker")
    r=sp.add_parser("record"); r.add_argument("json_file")
    q=sp.add_parser("audit"); q.add_argument("json_file")
    a=ap.parse_args()
    if a.cmd=="preflight": out=preflight(a.ticker)
    else:
        x=json.loads(Path(a.json_file).read_text())
        out=audit(x) if a.cmd=="audit" else record(x)
    print(json.dumps(out,indent=2))
if __name__=="__main__": main()
