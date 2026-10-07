"""Transparent exploratory ranking, not a calibrated prediction or entry gate."""
import re
from continuous.core import timestamp


def rank(data, now):
    s=data['snapshot']; rating=data['rating']; reasons=[]; missing=[]
    if not s.get('bar_time_utc'):
        return None
    try:
        fresh=0<=now-timestamp(s['bar_time_utc'])<=120
    except (KeyError,TypeError,ValueError):
        fresh=False
    move=s.get('change_pct'); price=s.get('price')
    if not fresh or data.get('trend_stale') or not price or move is None or not 0<move<8 or rating['classification'] in ('LATE','AVOID'):
        return None
    news=[n for n in data['news'] if n.get('confidence',0)>=55 and not n.get('negative') and not n.get('dilution') and re.search(r'beats?|rais\w*.*(?:guidance|outlook)|approval|contract|partnership|\bAI\b.*(?:cloud|demand|infrastructure)',n['title'],re.I)]
    score=0
    if news:
        score+=30; reasons.append('خبر محفّز مرصود: '+news[0]['title'])
    else:
        missing.append('محفّز إخباري موثوق')
    if 'الأكثر تداولًا' in data['trend_reasons']:
        score+=15; reasons.append('ضمن قائمة الأكثر تداولًا')
    if 'الأكثر ارتفاعًا' in data['trend_reasons']:
        score+=10; reasons.append('ضمن قائمة الأكثر ارتفاعًا')
    turnover=price*(s.get('cum_volume') or 0)
    if turnover>=5_000_000:
        score+=10; reasons.append('قيمة التداول اليومي ≥5 ملايين دولار؛ مؤشر نشاط وليس قياس فرق العرض والطلب')
    momentum=data.get('momentum_pct')
    if momentum is not None and momentum>=.2:
        score+=20; reasons.append('السعر تسارع بين لقطتين حديثتين: '+str(round(momentum,2))+'%')
    else:
        missing.append('تسارع سعر حديث ≥0.2% بين لقطتين')
    if 0<move<=4:
        score+=5; reasons.append('ارتفاع اليوم محدود نسبيًا ≤4%؛ ليس قياسًا منذ الخبر')
    missing.extend(['RVOL دقيقة واحدة موثوق','تأكيد اختراق وVWAP والسيولة المباشرة'])
    if score<30:
        return None
    return dict(ticker=data['ticker'],score=score,score_kind='ترتيب متابعة غير معاير، وليس احتمال صعود',
        price=price,reasons=reasons,missing=missing,
        label='مرشّح مع محفّز' if news else 'زخم مرصود — المحفّز غير مؤكد',buyable=rating['buyable'])
