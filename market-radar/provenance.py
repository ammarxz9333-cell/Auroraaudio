from urllib.parse import urlparse

def source_identity(source, item=None):
    u=(item or {}).get("url") or source.get("url") or ""
    host=urlparse(u).netloc.lower()
    return host.removeprefix("www.") or source.get("name","unknown").lower()

def catalyst_signature(item, catalysts):
    text=f"{item.get('title','')} {item.get('snippet','')}".lower()
    return sorted({p for p in catalysts if p not in {"8-k","form 8-k","form 4","13d","13g"} and p in text})

def related(sig_a, sig_b):
    return bool(set(sig_a) & set(sig_b))

def provenance(source,item,tickers,fetched,watchlist,match_watchlist,catalysts,now_utc):
    sig=catalyst_signature(item,catalysts)
    cp=item.get("published")
    own=source_identity(source,item)
    corroborators={}
    primary=[]
    ages=[]
    for osrc,oitems in fetched:
        for oi in oitems:
            oid=source_identity(osrc,oi)
            if oid==own: continue
            ots=match_watchlist(f"{oi.get('title','')} {oi.get('snippet','')}",watchlist)
            if not (set(ots)&set(tickers)): continue
            osig=catalyst_signature(oi,catalysts)
            if not related(sig,osig): continue
            op=oi.get("published")
            if cp is not None and op is not None:
                delta=abs((op-cp).total_seconds())
                if delta>21600: continue
                ages.append(delta)
            corroborators[oid]=osrc.get("name","unknown")
            if osrc.get("class")=="primary":
                primary.append(oid)
    def velocity(sec):
        return sum(1 for x in ages if x<=sec)
    return {"catalyst_signature":sig,"source_identity":own,"independent_corroboration_count":len(corroborators),"independent_corroborators":sorted(corroborators.values())[:8],"primary_confirmation":bool(primary),"rumor_only":False,"first_seen_utc":now_utc().isoformat(),"source_velocity":{"30m":velocity(1800),"2h":velocity(7200),"6h":len(corroborators)}}
