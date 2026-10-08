import importlib.util, pathlib, tempfile, csv
p=pathlib.Path(__file__).with_name("prelaunch_backtest.py")
s=importlib.util.spec_from_file_location("pb",p); m=importlib.util.module_from_spec(s); s.loader.exec_module(m)
rows=[{"event_date":"d","ticker":"A","family":"x","eventual_move_pct":"12","alert_move_pct":"2","source_to_alert_seconds":"30","false_positive":"false","duplicate":"false","direction_error":"false","tradability_label":"EARLY_TRADABLE"},
{"event_date":"d","ticker":"B","family":"x","eventual_move_pct":"15","alert_move_pct":"9","source_to_alert_seconds":"60","false_positive":"false","duplicate":"false","direction_error":"false","tradability_label":"LATE_AT_OPEN"}]
r=m.evaluate(rows)
assert r["recall_before_3pct"]==50.0
assert r["median_source_to_alert_seconds"]==45.0
print("ok")
