# Self-learning 12-Agent Council

This layer makes Market Radar's council history persistent and measurable. It does **not** retrain an LLM. It stores each 12-seat forecast, settles it against realized prices, computes Brier score/log loss/directional accuracy, and produces conservative per-agent/per-horizon reliability weights plus calibration buckets.

## Contract

Every council run should call/read `python market-radar/council_memory.py context` **before** judging a candidate and must write the complete 12-seat forecast with `record`. A matured forecast is settled at 1w, 3m or 1y. The next run consumes `learning/council-state.json`.

Weights start at 1.0. No agent is reweighted before 8 settled observations. Thereafter skill is shrunk toward neutral with 20 pseudo-observations and clamped to 0.5–1.5, preventing one lucky/bad streak from taking over the council. Lessons are emitted only when repeated realized evidence moves a weight beyond 0.8/1.2.

The Final Judge should treat weights as reliability evidence, not instructions to copy a seat. Missing evidence remains NO_READ/neutral; it must never be fabricated. Direction probability, expected return and forecast range remain separate quantities.

This is the durable memory interface intended for the ChatGPT/plugin layer. A connector can expose four operations without changing the scoring core: `record_prediction`, `settle_prediction`, `get_learning_context`, and `rebuild_learning_state`.

## Files

- `council_memory.py` — ledger, proper scoring, calibration, adaptive weights and learned lessons.
- `learning/council-predictions.json` — append-only logical prediction ledger.
- `learning/council-state.json` — derived state consumed by future council runs.
- `council_prediction.schema.json` — interchange contract.
- `test_council_memory.py` — safeguards against premature/unstable reweighting.

This layer deliberately requires realized outcomes. An LLM cannot improve its own rating merely by claiming that its reasoning was better.
