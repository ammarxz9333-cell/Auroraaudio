#!/usr/bin/env python3
"""Always-on Market Radar daemon.

Runs the existing lawful public-source scanner repeatedly on a persistent host.
This removes GitHub Actions' coarse schedule as the primary latency bottleneck.
It does NOT create a market-data entitlement; tape latency still depends on the
configured/public data source.
"""
from __future__ import annotations
import argparse, os, random, subprocess, sys, time
from datetime import datetime, timezone
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
RADAR=ROOT/"radar.py"

def run_once():
    started=datetime.now(timezone.utc).isoformat()
    p=subprocess.run([sys.executable,str(RADAR)],cwd=str(ROOT),env=os.environ.copy())
    print(f"[daemon] {started} exit={p.returncode}",flush=True)
    return p.returncode

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--interval",type=float,default=float(os.getenv("RADAR_DAEMON_INTERVAL","20")))
    ap.add_argument("--jitter",type=float,default=float(os.getenv("RADAR_DAEMON_JITTER","2")))
    ap.add_argument("--once",action="store_true")
    a=ap.parse_args()
    if a.interval < 10:
        raise SystemExit("interval must be >=10s to avoid hammering public sources")
    while True:
        run_once()
        if a.once: return
        time.sleep(max(10,a.interval+random.uniform(-a.jitter,a.jitter)))
if __name__=="__main__": main()
