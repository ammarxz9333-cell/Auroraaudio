#!/usr/bin/env python3
"""Council I/O bridge for ChatGPT/plugin clients.

Produces a compact learning context before a run and accepts a completed
12-agent JSON prediction afterwards. This keeps the reasoning client stateless
while the repository remains the durable source of truth.
"""
import argparse, json
from pathlib import Path
import importlib.util

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location("council_memory",HERE/"council_memory.py")
cm=importlib.util.module_from_spec(spec); spec.loader.exec_module(cm)

def preflight(ticker=None):
    c=cm.context()
    return {"ticker":ticker,"required_agents":list(cm.AGENTS),"horizons":list(cm.HORIZONS),
            "learning":c,
            "instructions":[
              "Run all 12 agents independently.",
              "Use learned weights as reliability evidence, not as replacement probabilities.",
              "Keep P(up), expected return and range separate.",
              "Do not fabricate missing evidence; emit NO_READ/neutral.",
              "After Final Judge, persist the complete prediction."
            ]}

def main():
    ap=argparse.ArgumentParser(); sp=ap.add_subparsers(dest="cmd",required=True)
    p=sp.add_parser("preflight"); p.add_argument("--ticker")
    r=sp.add_parser("record"); r.add_argument("json_file")
    a=ap.parse_args()
    print(json.dumps(preflight(a.ticker) if a.cmd=="preflight" else cm.record(json.loads(Path(a.json_file).read_text())),indent=2))
if __name__=="__main__": main()
