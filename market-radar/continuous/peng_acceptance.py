"""Separate real-data late rejection from synthetic early timing assertions."""
import json
import tempfile
from pathlib import Path
from continuous.core import Engine, timestamp, utc


def main():
    evidence=json.loads(Path(__file__).with_name('peng_evidence.json').read_text())
    publication=timestamp('2026-10-06T20:05:00Z')
    with tempfile.TemporaryDirectory() as tmp:
        e=Engine(tmp+'/radar.db',{'PENG':['Penguin Solutions']})
        previous=evidence['days']['2026-10-06']['last']
        samples=[previous]+evidence['regular_session_open_sample']
        classifications=[]
        for index,b in enumerate(samples):
            t=b['t']/1000+60
            e.bar(dict(ticker='PENG',time=utc(t),open=b['o'],high=b['h'],low=b['l'],close=b['c'],volume=b['v'],vwap=b['c'],spread_pct=None),t)
            if index==0:
                e.event(dict(title='Penguin Solutions raises FY2027 outlook on AI infrastructure demand',content='',url='https://ir.penguinsolutions.com/news/news-details/2026/Penguin-Solutions-Announces-Fourth-Quarter-and-Full-Year-Fiscal-2026-Results/',published_at=utc(publication)),dict(id='penguin-ir',public=True,official=True,confidence=95),publication+1)
            else:
                a=json.loads(e.db.execute('SELECT payload FROM outbox ORDER BY rowid DESC LIMIT 1').fetchone()[0])
                classifications.append({'bar_close':b['c'],'time':utc(t),'classification':a['classification'],'buyable':a['buyable']})
        assert classifications[-1]['bar_close']==73.74
        assert classifications[-1]['classification']=='LATE'
        assert classifications[-1]['buyable'] is False
        e.db.close()
    result={'real_regular_session_late_rejection':'PASS','classification_samples':classifications,
            'full_historical_acceptance':'INCOMPLETE','missing':['first-public timestamp proof','October 6 after-hours bars','preceding-session minute volume baselines','historical bid/ask'],
            'early_64_66_under_60s':'synthetic contract tests only; not proven historical detection',
            'live_delivery':'not configured'}
    print(json.dumps(result,indent=2))
    return result


if __name__=='__main__':main()
