import datetime as dt
import provenance

CAT={"merger":5,"contract award":5,"fda approved":5,"rumor":2}
WL={"ABC":["Acme"]}

def match(text,wl):
    return ["ABC"] if "acme" in text.lower() else []

def item(title,url,t):
    return {"title":title,"snippet":"","url":url,"published":t}

def test_unrelated_same_ticker_not_corroborated():
    t=dt.datetime(2026,1,1,tzinfo=dt.timezone.utc)
    src={"name":"Rumor A","class":"scoop","url":"https://a.example"}
    x=item("Acme merger rumor","https://a.example/x",t)
    other={"name":"Other","class":"primary","url":"https://b.example"}
    p=provenance.provenance(src,x,["ABC"],[(src,[x]),(other,[item("Acme contract award","https://b.example/y",t)])],WL,match,CAT,lambda:t)
    assert p["independent_corroboration_count"]==0
    assert p["primary_confirmation"] is False

def test_same_catalyst_independent_domain_confirms():
    t=dt.datetime(2026,1,1,tzinfo=dt.timezone.utc)
    src={"name":"Rumor A","class":"scoop","url":"https://a.example"}
    x=item("Acme merger rumor","https://a.example/x",t)
    primary={"name":"Official","class":"primary","url":"https://sec.gov"}
    p=provenance.provenance(src,x,["ABC"],[(src,[x]),(primary,[item("Acme merger","https://sec.gov/y",t+dt.timedelta(minutes=20))])],WL,match,CAT,lambda:t)
    assert p["independent_corroboration_count"]==1
    assert p["primary_confirmation"] is True
    assert p["source_velocity"]["30m"]==1

def test_same_domain_does_not_inflate():
    t=dt.datetime(2026,1,1,tzinfo=dt.timezone.utc)
    src={"name":"A","class":"scoop","url":"https://news.example"}
    x=item("Acme merger rumor","https://news.example/x",t)
    mirror={"name":"A mirror","class":"scoop","url":"https://news.example"}
    p=provenance.provenance(src,x,["ABC"],[(src,[x]),(mirror,[item("Acme merger","https://news.example/y",t)])],WL,match,CAT,lambda:t)
    assert p["independent_corroboration_count"]==0
