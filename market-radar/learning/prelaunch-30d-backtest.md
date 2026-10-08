# Pre-Launch Learning — 30-Day Forensic Backtest

Updated: 2026-10-08

## Objective

Optimize for information available before a stock has already made a large move. For each US-listed mover >=10%, reconstruct the point-in-time timeline and ask what was machine-detectable before +3%.

Do not train on hindsight-only facts. Separate predictable setup families from genuinely unpredictable shocks.

## Current baseline

The existing missed-movers audit shows severe advance-warning gaps:
- 2026-10-05: 27 eligible >=8% movers; 0 recent-alert overlap.
- 2026-10-07: 3 eligible >=8% movers; 0 recent-alert overlap.
- Overlap is not itself advance-warning recall because the day-gainers endpoint does not preserve first threshold-cross time.

## Pre-launch setup families

1. scheduled-binary-event
   - Clinical topline readout, PDUFA/FDA decision, advisory committee.
   - Create T-1/T-7 watch before outcome. Outcome remains binary; never infer direction from scheduling alone.
2. investor-day-guidance-repricing
   - Investor day, analyst day, long-range targets, major guidance revisions.
   - Parse presentation/release immediately; compare against consensus and prior guide.
3. confirmed-m-and-a
   - Definitive merger/tender offer/material 8-K.
   - Source-first detection from SEC/IR; technical confirmation is secondary.
4. credible-m-and-a-rumor
   - High-quality named-source reporting only. Label rumor provenance; require tape confirmation.
5. earnings-inflection
   - Large EPS/revenue surprise + guide raise/backlog/FCF inflection.
   - Score surprise magnitude before waiting for a large gap.
6. regulatory-document-first
   - FDA briefing documents, permits, certifications, government decisions.
   - Monitor original public documents because mainstream coverage can lag.
7. government-procurement-award
   - DoD/DOE/federal/state award or funded procurement with material contract value.
8. tape-first-unexplained-accumulation
   - First 1m/5m/15m abnormal same-time RVOL + relative strength + liquidity expansion while price is still roughly +0.5% to +3%.
   - This is a discovery trigger, not proof; immediately search original-source catalysts.
9. sector-sympathy
   - Primary catalyst in one issuer propagates to economically linked peers. Require relationship evidence and relative-strength confirmation.

## Point-in-time labels required for every historical mover

- earliest_public_timestamp
- price_at_first_public
- first_time_above_0_5pct / 1pct / 3pct / 5pct
- 1m/5m/15m/30m acceleration where available
- same-time RVOL
- VWAP state
- SPY/QQQ/sector relative strength
- float, short interest, options/liquidity context
- catalyst novelty and magnitude
- SEC/IR/FDA/government/mainstream-media timestamps
- balance sheet/runway and ATM/S-3/424B5 risk
- rumor provenance
- root cause of miss
- earliest point a lawful automated system could reasonably have alerted

## Root-cause taxonomy

source-coverage-gap; ticker-entity-extraction; catalyst-vocabulary-gap; score-threshold; watchlist-bias; source-latency; market-data-latency; deduplication; direction-classification; market-reaction-filter; sector-sympathy-not-modeled; analyst-action-not-modeled; index-flow-not-modeled; rumor-not-modeled; price-volume-momentum-not-modeled; genuinely-unpredictable.

## Candidate architecture change

T-1 Event Watchlist -> original-source catalyst listener -> dynamic HOT WATCHLIST -> 1m/tick tape monitor -> EARLY HEADS-UP -> VWAP/hold/volume -> CONFIRMED.

EARLY should target the first +0.5% to +3% reaction when catalyst magnitude and abnormal tape agree. Do not wait for +8% day-gainer status.

## Guardrails

- Never treat scheduled binary events as bullish before the result.
- Never use future information in historical scoring.
- Rumors must retain provenance and confidence.
- Closing-auction volume is not equivalent to organic accumulation.
- Reject changes that improve recall only by flooding alerts.
- Promote a rule to production only after walk-forward validation improves early recall with acceptable false-positive rate.
