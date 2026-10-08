import json,tempfile,unittest
from datetime import datetime,timezone
from pathlib import Path
from hot_watchlist import seed,load
from stream_tape import Tape

class T(unittest.TestCase):
 def test_ttl_and_early(self):
  with tempfile.TemporaryDirectory() as d:
   p=Path(d)/"h.json"; n=datetime(2026,10,8,14,0,tzinfo=timezone.utc)
   seed("XYZ","catalyst","earnings",ttl_minutes=30,path=p,now=n)
   self.assertIn("XYZ",load(p,n))
   tape=Tape()
   base=n
   tape.add({"ticker":"XYZ","ts":base.isoformat(),"price":100,"size":10,"prev_close":99})
   s=tape.add({"ticker":"XYZ","ts":base.replace(minute=5).isoformat(),"price":101,"size":10,"prev_close":99,"same_time_rvol":3})
   self.assertEqual(s["classification"],"EARLY_HEADS_UP")
if __name__=="__main__":unittest.main()
