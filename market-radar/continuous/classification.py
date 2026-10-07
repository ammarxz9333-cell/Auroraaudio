"""Explain the agreed rules, without treating public snapshots as streaming proof."""
from continuous.core import timestamp

LABELS = {
    'HIGH-CONVICTION EARLY': 'فرصة مبكرة قوية',
    'DEVELOPING': 'تتطور — راقب',
    'CONFIRMED': 'مؤكدة',
    'LATE': 'متأخرة — لا تطارد',
    'AVOID': 'تجنّب',
}


def classify(news, snapshot, alert, now):
    snapshot = snapshot or {}
    alert = alert or {}
    reasons = []
    risk = any(n.get('dilution') or n.get('negative') for n in news)
    fresh = False
    try:
        fresh = 0 <= now - timestamp(alert['detected_at']) <= 90 and not alert.get('tape_stale', True)
    except (KeyError, ValueError, TypeError):
        pass
    move = snapshot.get('since_event_move_pct')
    day_move = snapshot.get('change_pct')
    label = 'DEVELOPING'
    buyable = False
    if risk or (fresh and alert.get('classification') == 'AVOID'):
        label = 'AVOID'
        reasons.append('رُصد خبر سلبي أو تمويل قد يخفّف ملكية المساهمين؛ يلزم فحص المصدر.')
    elif (fresh and alert.get('classification') == 'LATE') or (move is not None and move >= 8):
        label = 'LATE'
        reasons.append('ارتفع السعر 8% أو أكثر منذ مرجع الخبر المرصود؛ الحركة متأخرة وفق القاعدة.')
    elif day_move is not None and day_move >= 8:
        label = 'LATE'
        reasons.append('ارتفع 8% أو أكثر عن الإغلاق السابق؛ تحذير مطاردة احترازي، وليس قياسًا منذ أول خبر.')
    elif fresh and alert.get('buyable') and alert.get('classification') in ('CONFIRMED','HIGH-CONVICTION EARLY') and alert.get('trigger') is not None and alert.get('invalidation') is not None:
        label = alert['classification']
        buyable = True
        reasons.append('اجتاز الخبر وشروط السعر والحجم المباشر والسيولة وحدّ المطاردة.')
    else:
        reasons.append('لم تكتمل شروط الفرصة المبكرة المؤكدة بحركة السوق.' if news else 'لا يوجد خبر محفّز مرصود لهذا السهم بعد.')
    if not fresh:
        reasons.append('تأكيد التداول المباشر غير متاح أو قديم؛ لقطة السعر العامة لا تفتح إشارة شراء.')
    if snapshot.get('holds_vwap') is False:
        reasons.append('السعر دون متوسط التداول المرجّح بالحجم في اللقطة العامة.')
    reasons.append('فحص SEC الكامل غير مكتمل؛ غياب خبر تمويل لا يثبت غياب المخاطر.')
    confidence = max((n.get('confidence',0) for n in news),default=0)
    return dict(classification=label,label=LABELS[label],buyable=buyable,
        decision='شراء مشروط' if buyable else 'لا تشتري الآن' if label in ('LATE','AVOID') else 'راقب — لا تدخل بعد',
        reasons=reasons,score=alert.get('score') if fresh else None,
        score_kind='درجة قواعد غير معايرة، وليست احتمال ربح',
        source_confidence=confidence,trigger=alert.get('trigger') if fresh else None,
        invalidation=alert.get('invalidation') if fresh else None,
        evidence={'stream_fresh':fresh,'rvol_1m':alert.get('rvol') if fresh else None,
            'acceleration':alert.get('acceleration') if fresh else None,
            'breakout':alert.get('breakout') if fresh else None,
            'relative_strength_pct':alert.get('relative_strength_pct') if fresh else None,
            'options':alert.get('options','unavailable') if fresh else 'unavailable',
            'independent_sources':len({n.get('source_group',n.get('source_id')) for n in news}),
            'official_source':any(n.get('official',False) for n in news)})
