from __future__ import annotations

import hashlib
import json
import math
import re
import sqlite3
from datetime import datetime, timezone
from zoneinfo import ZoneInfo
from continuous.reversal import detect_reversal

NY = ZoneInfo('America/New_York')
from urllib.parse import urlsplit


def timestamp(value):
    d = datetime.fromisoformat(value.replace('Z', '+00:00'))
    if d.tzinfo is None:
        raise ValueError('timezone required')
    return d.timestamp()


def utc(value):
    return datetime.fromtimestamp(value, timezone.utc).isoformat()


def number(value, positive=False):
    x = float(value)
    if not math.isfinite(x) or (positive and x <= 0):
        raise ValueError('invalid numeric value')
    return x


class Engine:
    def __init__(self, path, universe):
        self.universe = universe
        self.db = sqlite3.connect(path)
        self.db.row_factory = sqlite3.Row
        self.db.executescript('''
          PRAGMA journal_mode=WAL;
          CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY,ticker TEXT,story TEXT,published REAL,observed REAL,payload TEXT);
          CREATE INDEX IF NOT EXISTS events_ticker ON events(ticker,published);
          CREATE TABLE IF NOT EXISTS bars(ticker TEXT,time REAL,payload TEXT,PRIMARY KEY(ticker,time));
          CREATE TABLE IF NOT EXISTS outbox(id TEXT PRIMARY KEY,payload TEXT,sent INTEGER DEFAULT 0,attempts INTEGER DEFAULT 0,next_try REAL DEFAULT 0);
          CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT);
          CREATE TABLE IF NOT EXISTS technical_alerts(ticker TEXT,ny_day TEXT,classification TEXT,PRIMARY KEY(ticker,ny_day));
        ''')

    def resolve(self, text):
        found = set()
        explicit = {symbol.upper() for group in re.findall(r'\$([A-Z][A-Z0-9.\-]*)|(?:NASDAQ|NYSE)\s*:\s*([A-Z][A-Z0-9.\-]*)', text, re.I) for symbol in group if symbol}
        lower = text.lower()
        for ticker, aliases in self.universe.items():
            # Bare short English symbols like AI/IT/ON are ambiguous. Require cashtag,
            # exchange declaration or the verified company name.
            if ticker.upper() in explicit:
                found.add(ticker)
            if any(re.search(r'\b' + re.escape(a) + r'\b', text, re.I) for a in aliases if len(a) > 4 and a.lower() in lower):
                found.add(ticker)
        return sorted(found)

    def event(self, event, source, now):
        if source.get('public') is not True:
            raise ValueError('source is not approved public material')
        url = urlsplit(event['url'])
        if url.scheme not in ('https', 'http') or not url.hostname:
            raise ValueError('public provenance URL required')
        published = timestamp(event['published_at'])
        if published > now + 5:
            raise ValueError('future publication')
        text = event['title'] + ' ' + event.get('content', '')
        normalized = re.sub(r'\W+', ' ', text.lower()).strip()
        # Syndicated copies with the same headline are one story, independent of URL.
        story = hashlib.sha256(re.sub(r'\W+', ' ', event['title'].lower()).strip().encode()).hexdigest()[:20]
        for ticker in self.resolve(text):
            tokens = set(re.sub(r'\W+', ' ', event['title'].lower()).split())
            candidates = self.db.execute('SELECT story,payload FROM events WHERE ticker=? AND published>=?', (ticker, published-86400)).fetchall()
            for candidate in candidates:
                other = set(re.sub(r'\W+', ' ', json.loads(candidate['payload'])['title'].lower()).split())
                if len(tokens & other) / max(1, len(tokens | other)) >= .7:
                    story = candidate['story']
                    break
            payload = dict(event, source_id=source['id'], source_group=source.get('origin_group', source['id']),
                           confidence=source.get('confidence', 30), official=source.get('official', False),
                           dilution=bool(re.search(r'\b(offering|dilution|424b5|s-3|warrants|convertible|at.the.market)\b', text, re.I)),
                           negative=bool(re.search(r'\b(bankruptcy|fraud|lowers guidance|lowered guidance|failed trial|complete response letter)\b', normalized)))
            ident = hashlib.sha256((ticker + source['id'] + event['url'] + story).encode()).hexdigest()
            with self.db:
                self.db.execute('INSERT OR IGNORE INTO events VALUES(?,?,?,?,?,?)',
                                (ident, ticker, story, published, now, json.dumps(payload)))
            self.evaluate(ticker, now)

    def bar(self, bar, now):
        ticker = bar['ticker']
        if ticker not in self.universe:
            return
        t = timestamp(bar['time'])
        if t > now + 5 or now - t > 120:
            raise ValueError('stale/future live tape')
        for key in ('open', 'high', 'low', 'close', 'vwap'):
            bar[key] = number(bar[key], True)
        bar['volume'] = number(bar['volume'])
        if bar['volume'] < 0 or not (bar['low'] <= min(bar['open'], bar['close']) <= max(bar['open'], bar['close']) <= bar['high']):
            raise ValueError('invalid OHLCV')
        if bar.get('baseline_volume') is not None:
            number(bar['baseline_volume'], True)
            if timestamp(bar['baseline_asof']) >= t:
                raise ValueError('baseline lookahead')
        with self.db:
            self.db.execute('INSERT OR REPLACE INTO bars VALUES(?,?,?)', (ticker, t, json.dumps(bar)))
        # An abnormal volume minute is not a verified news event.
        self.evaluate(ticker, now)
        self.evaluate_reversal(ticker, now, t)

    def evaluate_reversal(self, ticker, now, bar_time):
        """Point-in-time technical watcher, separate from Overkill v2."""
        local = datetime.fromtimestamp(bar_time, NY)
        minute = local.hour * 60 + local.minute
        if not (9 * 60 + 30 < minute <= 16 * 60) or now - bar_time > 90:
            return
        session_start = local.replace(hour=9, minute=30, second=0, microsecond=0).timestamp()
        rows = self.db.execute(
            'SELECT payload FROM bars WHERE ticker=? AND time>? AND time<=? ORDER BY time',
            (ticker, session_start, bar_time)).fetchall()
        bars = [json.loads(r['payload']) for r in rows]
        signal = detect_reversal(bars)
        if not signal:
            return
        recent_news = self.db.execute(
            'SELECT payload FROM events WHERE ticker=? AND published BETWEEN ? AND ?',
            (ticker, now - 86400, now)).fetchall()
        if any(e.get('negative') or e.get('dilution')
               for row in recent_news for e in [json.loads(row['payload'])]):
            return
        day = local.date().isoformat()
        prior = self.db.execute(
            'SELECT classification FROM technical_alerts WHERE ticker=? AND ny_day=?',
            (ticker, day)).fetchone()
        strength = {'WATCH EARLY': 1, 'DEVELOPING': 2}
        if prior and strength.get(prior['classification'], 0) >= strength[signal['classification']]:
            return
        alert = dict(
            ticker=ticker, classification=signal['classification'],
            strategy='EARLY_TECHNICAL_REVERSAL', score=None,
            confidence_kind='rules-based, uncalibrated; not an Overkill BUY',
            first_public_at=None, first_observed_at=utc(bar_time),
            detected_at=utc(now), source_to_detection_seconds=None,
            price=signal['price'], price_at_source=None,
            price_move_since_source_pct=None, bounce_from_low_pct=signal['bounce_from_low_pct'],
            session_low=signal['session_low'], rvol=signal['rvol_5m'],
            rvol_basis=signal['rvol_basis'], acceleration=signal['change_5m_pct'],
            change_15m_pct=signal['change_15m_pct'], gap_pct=None,
            vwap=signal['vwap'], breakout=signal['compression_breakout'],
            relative_strength_pct=bars[-1].get('relative_strength_pct'),
            options='unavailable', catalyst='Technical reversal only; no new public catalyst confirmed',
            provenance=[{'source': bars[-1].get('feed', 'Alpaca stream; feed unknown'),
                         'url':'https://docs.alpaca.markets/docs/real-time-stock-pricing-data'}],
            source_confidence=None, independent_sources=0, novelty=False,
            dilution_sec_risk='unknown; full SEC issuer filing review required',
            trigger=signal['trigger'], invalidation=signal['invalidation'],
            chase_distance_pct=signal['chase_distance_pct'],
            buyable=False, tape_stale=False,
            evidence=signal, limitations=signal['limitations'])
        ident = hashlib.sha256(f'{ticker}:{day}:early:{signal["classification"]}'.encode()).hexdigest()
        with self.db:
            self.db.execute('INSERT OR IGNORE INTO outbox(id,payload) VALUES(?,?)',
                            (ident, json.dumps(alert)))
            self.db.execute(
                'INSERT OR REPLACE INTO technical_alerts(ticker,ny_day,classification) VALUES(?,?,?)',
                (ticker, day, signal['classification']))

    def evaluate(self, ticker, now):
        rows = self.db.execute('SELECT * FROM events WHERE ticker=? AND published BETWEEN ? AND ? ORDER BY published', (ticker, now - 86400, now)).fetchall()
        if not rows:
            return
        recent = self.db.execute('SELECT * FROM bars WHERE ticker=? AND time<=? ORDER BY time DESC LIMIT 60', (ticker, now)).fetchall()
        if not recent:
            for row in rows:
                key = f"{ticker}:{row['story']}"
                if self.db.execute('SELECT 1 FROM meta WHERE key=?', (key,)).fetchone():
                    continue
                event = json.loads(row['payload'])
                label = 'AVOID' if event['dilution'] or event['negative'] else 'DEVELOPING'
                alert = dict(ticker=ticker, classification=label, score=round(event['confidence']*.55),
                    first_public_at=utc(row['published']), first_observed_at=utc(row['observed']), detected_at=utc(now),
                    source_to_detection_seconds=now-row['published'], price=None,price_at_source=None,price_move_since_source_pct=None,
                    rvol=None,acceleration=None,gap_pct=None,vwap=None,breakout=False,relative_strength_pct=None,
                    catalyst=event['title'],source_confidence=event['confidence'],independent_sources=1,novelty=True,
                    trigger=None,invalidation=None,chase_distance_pct=None,buyable=False,tape_stale=True,
                    dilution_sec_risk='detected' if label=='AVOID' else 'unknown',options='unavailable',
                    provenance=[{'source':event['source_id'],'url':event['url']}],confidence_kind='heuristic, not calibrated probability')
                with self.db:
                    self.db.execute('INSERT INTO outbox(id,payload) VALUES(?,?)', (hashlib.sha256((key+label+str(now)).encode()).hexdigest(),json.dumps(alert)))
                    self.db.execute('INSERT INTO meta VALUES(?,?)', (key,label))
            return
        latest = recent[0]
        b = json.loads(latest['payload'])
        for story in dict.fromkeys(row['story'] for row in rows):
            group = [r for r in rows if r['story'] == story]
            first = group[0]
            events = [json.loads(r['payload']) for r in group]
            anchor_row = self.db.execute('SELECT * FROM bars WHERE ticker=? AND time<=? ORDER BY time DESC LIMIT 1', (ticker, first['published'])).fetchone()
            anchor = json.loads(anchor_row['payload'])['close'] if anchor_row and first['published'] - anchor_row['time'] <= 86400 else None
            anchor_fresh = anchor_row is not None and first['published'] - anchor_row['time'] <= 120
            price = b['close']
            move = (price / anchor - 1) * 100 if anchor else None
            rvol = b['volume'] / b['baseline_volume'] if b.get('baseline_volume') and b.get('baseline_days', 0) >= 5 else None
            prev = json.loads(recent[1]['payload']) if len(recent) > 1 else None
            acceleration = b['volume'] / prev['volume'] if prev and prev['volume'] > 0 else None
            prior_high = max((json.loads(r['payload'])['high'] for r in recent[1:] if latest['time'] - r['time'] <= 1200), default=None)
            breakout = prior_high is not None and price > prior_high
            spread = b.get('spread_pct')
            stale = now - latest['time'] > 90
            source_conf = max(number(e['confidence']) for e in events)
            independent = len({e['source_group'] for e in events})
            official = any(e['official'] for e in events)
            catalyst = any(re.search(r'beat|rais\w*.*(?:guidance|outlook)|ai.*(?:demand|infrastructure|factory)|merger|acquisition|contract|approval|partnership|earnings|8-k', e['title']+' '+e.get('content',''), re.I) for e in events)
            risk = any(json.loads(r['payload'])['dilution'] or json.loads(r['payload'])['negative'] for r in rows)
            ny_bar = datetime.fromtimestamp(latest['time'], NY)
            bar_minute = ny_bar.hour * 60 + ny_bar.minute
            tape = (rvol is not None and rvol >= 2 and price >= b['vwap'] and breakout
                    and b.get('market_session') == 'REGULAR'
                    and 9 * 60 + 30 < bar_minute <= 16 * 60)
            score = min(100, round(source_conf * .55 + (20 if tape else 0) + (10 if acceleration and acceleration >= 2 else 0) + (10 if independent >= 2 else 0) + (5 if now - first['published'] <= 600 else 0)))
            label = 'DEVELOPING'
            buyable = False
            if risk:
                label = 'AVOID'
            elif move is not None and (move >= 8 or (price / b['vwap'] - 1) * 100 >= 5):
                label = 'LATE'
            elif catalyst and not stale and anchor_fresh and anchor and tape and spread is not None and 0 <= number(spread) <= 1 and source_conf >= 70 and score >= 70 and now - first['published'] <= 600:
                label = 'CONFIRMED' if official else 'HIGH-CONVICTION EARLY'
                buyable = True
            alert = dict(ticker=ticker, classification=label, score=score, confidence_kind='heuristic, not calibrated probability',
                         first_public_at=utc(first['published']), first_observed_at=utc(first['observed']),
                         detected_at=utc(now), source_to_detection_seconds=round(now-first['published'], 3),
                         price=price, price_at_source=anchor, price_at_source_basis='last completed bar before publication', source_price_fresh=anchor_fresh, price_move_since_source_pct=move,
                         rvol=rvol, rvol_basis='same NY minute median, preceding sessions' if rvol else 'unavailable',
                         acceleration=acceleration, gap_pct=b.get('gap_pct'), vwap=b['vwap'], breakout=breakout,
                         relative_strength_pct=b.get('relative_strength_pct'), options=b.get('options', 'unavailable'),
                         catalyst=events[0]['title'], provenance=[{'source': e['source_id'], 'url': e['url']} for e in events],
                         source_confidence=source_conf, independent_sources=independent, novelty=independent == 1,
                         dilution_sec_risk='detected' if risk else 'unknown; full issuer filing review required',
                         trigger=prior_high, invalidation=b['vwap'], chase_distance_pct=move,
                         buyable=buyable, tape_stale=stale,
                         limitations=['earliest observed monitored source, not proof of earliest public appearance', 'SEC risk unknown may conceal financing risk'])
            # One notification per state transition. Price updates alone do not spam.
            key = f'{ticker}:{story}'
            last = self.db.execute('SELECT value FROM meta WHERE key=?', (key,)).fetchone()
            if last and last['value'] == label:
                continue
            ident = hashlib.sha256((key + label + str(now)).encode()).hexdigest()
            with self.db:
                self.db.execute('INSERT INTO outbox(id,payload) VALUES(?,?)', (ident, json.dumps(alert)))
                self.db.execute('INSERT OR REPLACE INTO meta VALUES(?,?)', (key, label))
        with self.db:
            self.db.execute('DELETE FROM bars WHERE time<?', (now - 172800,))
            self.db.execute('DELETE FROM events WHERE published<?', (now - 172800,))


def format_alert(a):
    if a.get('strategy') == 'EARLY_TECHNICAL_REVERSAL':
        e = a['evidence']
        def val(v):
            return 'غير متاح' if v is None else f'{v:.2f}'
        return '\n'.join([
            f"🔎 {a['classification']} — ${a['ticker']} | انعكاس فني مبكر",
            f"وقت الفحص: {a['detected_at']} UTC | سعر آخر شمعة مكتملة: ${val(a['price'])}",
            f"من قاع الجلسة ${val(e['session_low'])}: +{val(e['bounce_from_low_pct'])}%",
            f"5m: +{val(e['change_5m_pct'])}% | 15m: +{val(e['change_15m_pct'])}% | قيعان 5m/15m صاعدة",
            f"RVOL 5m: {val(e['rvol_5m'])}× (مقابل الدقائق نفسها في جلسات سابقة) | VWAP: ${val(e['vwap'])}",
            f"كسر نطاق: {'نعم' if e['compression_breakout'] else 'لا'} | استعادة VWAP: {'نعم' if e['vwap_reclaim'] else 'لا'}",
            f"قوة نسبية مقابل SPY: {val(a.get('relative_strength_pct'))} نقطة مئوية",
            f"مستوى متابعة: ${val(a['trigger'])} | ضعف/إبطال أولي: ${val(a['invalidation'])}",
            f"البعد عن VWAP: {val(a['chase_distance_pct'])}% | خطر المطاردة يزداد مع اتساع الحركة",
            "لا يوجد محفز جديد مؤكد؛ مخاطر التمويل/SEC غير محسومة.",
            "Overkill v2 اليومية: غير محسوبة بهذه الإشارة؛ WATCH/DEVELOPING ليست توصية شراء.",
            "BUYABLE_NOW: NO — لا توجد أوامر تداول آلية.",
        ])[:4000]
    if a.get('strategy') == 'OVERKILL_V2':
        fusion = a.get('fusion') or {}
        components = fusion.get('components') or {}
        context = a.get('price_context') or {}
        actions = ['• ' + mode + ': ' +
                   ('شراء افتراضي عند الافتتاح التالي' if action == 'BUY_NEXT_OPEN'
                    else 'خروج افتراضي عند الافتتاح التالي')
                   for mode, action in a['actions'].items()]
        score = fusion.get('score')
        score_text = (f"{score}/100" if score is not None else
                      'غير مطبق على إشارات الخروج')
        lines = [
            ('🔺 ترقية تقييم الإشارة' if a.get('upgrade')
             else '📊 إشارة Overkill + Market Radar'),
            '$' + a['ticker'] + ' | ' + fusion.get('tier', 'UNRATED'),
            'تقييم الأدلة: ' + score_text + ' (ليس احتمال ربح)',
            'إغلاق الإشارة: ' + a['signal_date'],
            'الافتتاح الافتراضي التالي: ' + a['next_expected_open_date'] +
            ' (نيويورك)',
            *actions,
        ]
        if components:
            lines.append(
                'النقاط: فني ' + str(components.get('technical')) +
                ' + أخبار ' + str(components.get('positive_news')) +
                ' + زخم ' + str(components.get('momentum')) +
                ' - مخاطر ' + str(components.get('risk_penalty')))
        if context.get('last_completed_close') is not None:
            lines.append('سعر آخر إغلاق مكتمل: $' +
                         f"{context['last_completed_close']:.2f}" +
                         ' (ليس سعر تنفيذ)')
        if context.get('last_completed_low') is not None:
            lines.append('قاع آخر شمعة: $' +
                         f"{context['last_completed_low']:.2f}" +
                         ' (مرجع للمخاطر، ليس وقف خسارة محدداً)')
        catalyst = fusion.get('positive_catalyst')
        if catalyst:
            lines.append('خبر إيجابي موثق: ' + catalyst['title'])
            lines.append(catalyst['url'])
        for bad in (fusion.get('risk_headlines') or [])[:2]:
            lines.append('تحذير ' + bad['kind'] + ': ' + bad['title'])
            lines.append(bad['url'])
        if fusion.get('missing_evidence'):
            lines.append('أدلة غير متوفرة: ' +
                         '، '.join(fusion['missing_evidence'][:2]))
        lines.extend([
            'لا يوجد هدف ربح ثابت 10%؛ الخروج بإشارة المؤشر.',
            'لا توجد أوامر تداول آلية؛ المراكز افتراضية.',
            'المؤشر إعادة بناء مستقلة، وليس نسخة TikTok الأصلية.'
        ])
        return '\n'.join(lines)[:4000]
    def fmt(v):
        return 'unknown' if v is None else f'{v:.2f}' if isinstance(v, (float, int)) else str(v)
    return '\n'.join([
        f"{a['classification']} — ${a['ticker']} | score {a['score']}/100 (heuristic)",
        f"Public: {a['first_public_at'] or 'unverified (technical only)'} | detected: {a['detected_at']}",
        f"Price ${fmt(a['price'])} | at source ${fmt(a['price_at_source'])} | move {fmt(a['price_move_since_source_pct'])}% | rebound from low {fmt(a.get('bounce_from_low_pct'))}%",
        f"RVOL {fmt(a['rvol'])} ({a.get('rvol_basis', 'unspecified')}) | acceleration {fmt(a['acceleration'])} | gap {fmt(a['gap_pct'])}%",
        f"VWAP {fmt(a['vwap'])} | breakout {a['breakout']} | RS {fmt(a['relative_strength_pct'])}",
        a['catalyst'][:600],
        f"Source confidence {a['source_confidence']} | independent {a['independent_sources']} | novelty {a['novelty']}",
        f"Trigger ${fmt(a['trigger'])} | invalidation ${fmt(a['invalidation'])} | chase {fmt(a['chase_distance_pct'])}%",
        f"Buyable by rule: {a['buyable']} | SEC/dilution: {a['dilution_sec_risk']} | options {a['options']}",
        *[p['source'] + ': ' + p['url'] for p in a['provenance'][:3]],
    ])[:4000]
