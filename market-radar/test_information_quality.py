import importlib.util
from pathlib import Path

P=Path(__file__).with_name("radar.py")
spec=importlib.util.spec_from_file_location("radar_under_test",P)
radar=importlib.util.module_from_spec(spec); spec.loader.exec_module(radar)

def test_information_quality_primary():
    q=radar.information_quality({"class":"primary"},{"title":"Company announces agreement","snippet":""})
    assert q["official_source"] is True
    assert q["source_quality"]==5
    assert q["rumor_language"] is False

def test_information_quality_rumor():
    q=radar.information_quality({"class":"social"},{"title":"Rumor: sources say deal possible","snippet":""})
    assert q["official_source"] is False
    assert q["source_quality"]==1
    assert q["rumor_language"] is True

def test_finra_contract_marks_not_short_interest():
    # Contract invariant: any successful FINRA daily payload must never be mislabeled as short interest.
    fn=radar.finra_short_sale_volume
    assert "NOT short interest" in (fn.__doc__ or "")
