from __future__ import annotations

import hashlib
import json
import math
import re
import sqlite3
from datetime import datetime, timezone
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
        ''')

    def resolve(self, text):
        found = set()
        for ticker, aliases in self.universe.items():
            # Bare short English symbols like AI/IT/ON are ambiguous. Require cashtag,
            # exchange declaration or the verified company name.
            if re.search(r'\$' + re.escape(ticker) + r'\b|(?:NASDAQ|NYSE)\s*:\s*' + re.escape(ticker) + r'\b', text, re.I):
                found.add(ticker)
            if any(re.search(r'\b' + re.escape(a) + r'\b', text, re.I) for a in aliases if len(a) > 4):
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
        prior = self.db.execute('SELECT payload FROM bars WHERE ticker=? AND time<? ORDER BY time DESC LIMIT 1',(ticker,t)).fetchone()
        baseline = bar.get('baseline_volume')
        prior_volume = json.loads(prior['payload'])['volume'] if prior else 0
        known = self.db.execute('SELECT 1 FROM events WHERE ticker=? AND published>? LIMIT 1',(ticker,now-86400)).fetchone()
        if not known and baseline and bar.get('baseline_days',0)>=5 and bar['volume']/baseline>=4 and prior_volume>0 and bar['volume']/prior_volume>=2:
            # Tape-first discovery is explicitly unconfirmed, never a fabricated news catalyst.
            self.event(dict(title=f'Unconfirmed tape anomaly ${ticker}', content='Investigate public catalyst; abnormal volume acceleration',
                            url='https://docs.alpaca.markets/us/docs/real-time-stock-pricing-data',published_at=utc(t)),
                       dict(id='tape-derived',public=True,confidence=0,official=False),now)
        self.evaluate(ticker, now)

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
            tape = rvol is not None and rvol >= 2 and price >= b['vwap'] and breakout
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
    def fmt(v):
        return 'unknown' if v is None else f'{v:.2f}' if isinstance(v, (float, int)) else str(v)
    return '\n'.join([
        f"{a['classification']} — ${a['ticker']} | score {a['score']}/100 (heuristic)",
        f"Public: {a['first_public_at']} | detected: {a['detected_at']}",
        f"Price ${fmt(a['price'])} | at source ${fmt(a['price_at_source'])} | move {fmt(a['price_move_since_source_pct'])}%",
        f"RVOL {fmt(a['rvol'])} | acceleration {fmt(a['acceleration'])} | gap {fmt(a['gap_pct'])}%",
        f"VWAP {fmt(a['vwap'])} | breakout {a['breakout']} | RS {fmt(a['relative_strength_pct'])}",
        a['catalyst'][:600],
        f"Source confidence {a['source_confidence']} | independent {a['independent_sources']} | novelty {a['novelty']}",
        f"Trigger ${fmt(a['trigger'])} | invalidation ${fmt(a['invalidation'])} | chase {fmt(a['chase_distance_pct'])}%",
        f"Buyable by rule: {a['buyable']} | SEC/dilution: {a['dilution_sec_risk']} | options {a['options']}",
        *[p['source'] + ': ' + p['url'] for p in a['provenance'][:3]],
    ])[:4000]
