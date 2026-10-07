"""Evidence-based decision memo; missing evidence remains explicit."""
from continuous.core import utc, timestamp


def analyze(stock, financials, now):
    snapshot=stock.get('snapshot') or {}; rating=stock['rating']; news=[]
    for item in stock['news']:
        try:
            if 0<=now-timestamp(item['published_at'])<=86400:
                news.append(item)
        except (KeyError,TypeError,ValueError,AttributeError):
            continue
    missing=['النقد والديون والتدفق النقدي غير متاحة','مراجعة ملفات SEC والتخفيف غير مكتملة','مقارنة التقييم بشركات القطاع غير مكتملة']
    try:
        fresh=0<=now-timestamp(snapshot['bar_time_utc'])<=120
    except (KeyError,TypeError,ValueError,AttributeError):
        fresh=False
    label=rating['classification']
    advice='انتظار — لا دخول الآن'
    reason='تأكيد الدخول المباشر أو البيانات الأساسية غير مكتمل.'
    if label=='AVOID':
        advice='تجنّب حاليًا'; reason='رُصد خطر إخباري أو تمويل؛ يلزم فحص الوثيقة الأصلية.'
    elif label=='LATE':
        advice='لا تطارد؛ انتظر فرصة جديدة'; reason='الحركة متأخرة وفق قاعدة الرادار، وليست حكمًا على القيمة العادلة للشركة.'
    elif rating.get('buyable') and fresh:
        advice='دخول فني مشروط — مراجعة المخاطر لازمة'; reason='اجتازت الإشارة بوابات الدخول الفني؛ التحليل المالي وSEC غير مكتملين فلا أعتبرها توصية شراء مكتملة.'
    if not fresh:
        missing.append('السعر غير متاح أو قديم')
    if not news:
        missing.append('لا يوجد محفّز إخباري مطابق مرصود')
    earnings=financials.get('epsTrailingTwelveMonths')
    valuation=[]
    if earnings is not None:
        valuation.append('ربحية آخر 12 شهرًا سالبة؛ مضاعف الربحية التقليدي لا يصلح للحكم بأنه رخيص.' if earnings<=0 else 'ربحية آخر 12 شهرًا موجبة؛ لا تكفي وحدها لإثبات جودة الأرباح.')
    if financials.get('forwardPE') is not None:
        valuation.append('المضاعف المتوقع يعتمد على تقديرات قد تتغير؛ ليس قيمة عادلة أو ضمانًا للنمو.')
    if not financials:
        valuation.append('بيانات التقييم المالي غير متاحة من القوائم الحالية.')
    technical=[]
    if snapshot.get('holds_vwap') is not None:
        technical.append('السعر فوق VWAP في اللقطة العامة.' if snapshot['holds_vwap'] else 'السعر دون VWAP؛ انتظار استعادة القوة قبل التفكير بالدخول.')
    if snapshot.get('same_time_volume_ratio') is not None:
        technical.append('نسبة الحجم إلى نفس الوقت من تاريخ Yahoo المحدود: '+str(snapshot['same_time_volume_ratio'])+'؛ ليست RVOL بث الدقيقة.')
    technical.extend(rating['reasons'])
    return dict(ticker=stock['ticker'],name=stock['name'],analyzed_at=utc(now),advice=advice,reason=reason,
        completeness='تحليل بالبيانات المتاحة — مراجعة مالية وSEC ناقصة',
        price=snapshot.get('price'),price_asof=snapshot.get('bar_time_utc'),
        catalyst={'headlines':news,'assessment':'لا يوجد خبر مطابق خلال آخر 24 ساعة.' if not news else 'مصدر رسمي مرصود' if any(n.get('official') for n in news) else 'العناوين تحتاج تأكيدًا من المصدر الأصلي؛ لا تثبت مفاجأة أرباح أو قيمة العقد'},
        technical={'observations':technical,'change_pct':snapshot.get('change_pct'),'change_5m_pct':snapshot.get('change_5m_pct'),'change_15m_pct':snapshot.get('change_15m_pct'),'vwap':snapshot.get('vwap'),'session_high':snapshot.get('session_high'),'session_low':snapshot.get('session_low')},
        valuation={'metrics':financials,'interpretation':valuation,'source':'Yahoo public screener, provider-reported multiples and EPS'},
        scenarios={'positive':'تأكيد المحفّز من الشركة مع تسارع حجم موثوق واختراق يتماسك فوق VWAP.','negative':'فشل استعادة VWAP أو كسر مستوى الإلغاء أو ظهور تمويل/خبر ينفي المحفّز.','entry':rating.get('trigger'),'invalidation':rating.get('invalidation'),'target':None,'risk_reward':None},
        missing=missing,limits='ليست نصيحة شخصية أو تقدير احتمال ربح؛ لا توجد قيمة عادلة أو هدف سعر مثبت. سجل الترشيحات لا يثبت نجاح التوقعات.')
