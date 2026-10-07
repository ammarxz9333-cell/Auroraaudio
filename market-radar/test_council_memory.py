import importlib.util, tempfile
from pathlib import Path
spec=importlib.util.spec_from_file_location("cm",Path(__file__).parent/"council_memory.py")
cm=importlib.util.module_from_spec(spec); spec.loader.exec_module(cm)

def sample(p=.7):
    return {"ticker":"TEST","price":100,"agents":{a:{"p_up":{"1w":p,"3m":p,"1y":p}} for a in cm.AGENTS},
            "final":{"p_up":{"1w":p,"3m":p,"1y":p}}}

def test_scores():
    assert round(cm.brier(.8,1),2)==.04
    assert cm.logloss(.8,1)>0

def test_learning_requires_samples():
    with tempfile.TemporaryDirectory() as d:
        cm.LEDGER=Path(d)/"ledger.json"; cm.STATE=Path(d)/"state.json"
        rows=[]
        for i in range(7):
            x=sample(.9); x["id"]=str(i); x["outcomes"]={"1w":{"up":0,"return":-.1}}
            rows.append(x)
        s=cm.rebuild_state(rows)
        assert s["agents"]["news"]["1w"]["weight"]==1.0

def test_persistent_bad_agent_downweights():
    rows=[]
    for i in range(30):
        x=sample(.55); x["id"]=str(i); x["outcomes"]={"1w":{"up":1,"return":.02}}
        x["agents"]["news"]["p_up"]["1w"]=.05
        rows.append(x)
    with tempfile.TemporaryDirectory() as d:
        cm.STATE=Path(d)/"state.json"
        s=cm.rebuild_state(rows)
        assert s["agents"]["news"]["1w"]["weight"]<1.0


def test_roundtrip_record_then_context():
    with tempfile.TemporaryDirectory() as d:
        cm.LEDGER=Path(d)/"ledger.json"; cm.STATE=Path(d)/"state.json"
        row=cm.record(sample(.63))
        saved=cm._load(cm.LEDGER,[])
        assert len(saved)==1
        assert saved[0]["id"]==row["id"]
        ctx=cm.context()
        assert ctx["agent_weights"]["price_chart"]["1w"]==1.0
        assert ctx["agent_weights"]["news"]["3m"]==1.0
