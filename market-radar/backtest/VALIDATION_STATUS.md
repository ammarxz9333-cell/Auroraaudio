# Market Radar validation status — 2026-09-28

The September 21–25 cases are **retrospective replays**. Pilot decision files were committed on September 27, after their market sessions. The repository's “frozen before outcome” label means only that the decision files preceded the later reveal files in Git. It does not establish that those decisions existed before the market movement.

The reported 17/18 (94.44%) event-session directional alignment is descriptive and must not be called a prospective hit rate or trading win rate. The 10/10 abstention audit is also retrospective and contains no executable buy entries. Daily open/high/low/close cannot prove which of +5% and -5% occurred first when both were touched.

From schema v2, `pit_backtester.py` records the actual freeze time in the locked record. Only records frozen no later than their cutoff can contribute to `prospective_directional_accuracy_pct`. Legacy records and later historical replays remain available for failure analysis but are excluded from that prospective metric.

Current prospective trading precision: **unmeasured**. Report the number of timestamped live BUYABLE_NOW entries, their first executable prices, ordered intraday threshold paths, losses, ambiguous outcomes, and abstentions before presenting any success percentage.
