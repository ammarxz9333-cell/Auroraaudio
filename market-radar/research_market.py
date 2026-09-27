def relative_strength(stock, spy=None, qqq=None):
    def move(x):
        if not x or x.get("previous_close") in (None,0) or x.get("price") is None:
            return None
        return (float(x["price"])/float(x["previous_close"])-1)*100
    sm=move(stock); pm=move(spy); qm=move(qqq)
    return {"stock_move_pct":round(sm,3) if sm is not None else None,"vs_spy_pct":round(sm-pm,3) if sm is not None and pm is not None else None,"vs_qqq_pct":round(sm-qm,3) if sm is not None and qm is not None else None}

def possible_halt(bars):
    reg=[b for b in bars if 570 <= b["ny"].hour*60+b["ny"].minute < 960]
    if len(reg)<2: return {"possible_halt":False,"gap_minutes":None,"minutes_since_possible_resume":None}
    gaps=[]
    for a,b in zip(reg,reg[1:]):
        mins=(b["utc"]-a["utc"]).total_seconds()/60
        if mins>10: gaps.append((mins,b["utc"]))
    if not gaps: return {"possible_halt":False,"gap_minutes":None,"minutes_since_possible_resume":None}
    mins,resume=gaps[-1]; now=reg[-1]["utc"]
    return {"possible_halt":True,"gap_minutes":round(mins,1),"possible_resume_utc":resume.isoformat(),"minutes_since_possible_resume":round((now-resume).total_seconds()/60,1)}

def timeframe_features(bars):
    reg=[b for b in bars if 570 <= b["ny"].hour*60+b["ny"].minute < 960]
    if not reg: return {}
    last=reg[-1]["close"]
    def ret(n):
        return round((last/reg[-n]["close"]-1)*100,3) if len(reg)>=n and reg[-n]["close"] else None
    prior_high=max((b["high"] for b in reg[:-1]),default=None)
    return {"return_15m_pct":ret(4),"return_30m_pct":ret(7),"return_60m_pct":ret(13),"above_prior_intraday_high":bool(prior_high is not None and last>prior_high),"prior_intraday_high":round(prior_high,4) if prior_high is not None else None}
