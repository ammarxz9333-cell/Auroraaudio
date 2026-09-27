# Market Radar 100-case PIT protocol

1. Candidate universe is selected independently of the classifier and includes positive movers, negative movers and controls.
2. For every case, choose a cutoff timestamp before the evaluation horizon.
3. Snapshot file may contain only information public by cutoff: primary/secondary evidence timestamps, price/volume state, catalyst features, financing context and broad-market context.
4. Outcome/forward-return fields are stored separately and are prohibited from snapshot files.
5. Freeze each decision with pit_backtester.py. The SHA-256 decision lock must exist before outcome reveal.
6. Reveal outcomes only after all decisions in the batch are frozen.
7. Report directional accuracy and bullish precision, plus coverage, false positives, misses and late/no-trade classifications.
8. Never tune classifier rules on validation outcomes. Any rule change starts a new validation version.

Important: a historical ranking page may be used to construct the candidate universe, but its future-return columns must never be copied into classifier snapshots.
