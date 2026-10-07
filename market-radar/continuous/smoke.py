"""Run real public feed listeners for a bounded time; never configure Telegram here."""
import asyncio
import json
import os
import sys
import tempfile
from pathlib import Path
from continuous.service import Service


async def check():
    config=json.loads(Path('continuous/sources.json').read_text())
    with tempfile.TemporaryDirectory() as temp:
        service=Service(config,temp+'/smoke.db')
        os.environ['RADAR_PORT']='18787'
        task=asyncio.create_task(service.run())
        try:
            await asyncio.sleep(20)
            report={'adapters':service.status,'events_observed':service.engine.db.execute('SELECT count(*) FROM events').fetchone()[0],
                    'outbox_pending':service.engine.db.execute('SELECT count(*) FROM outbox WHERE sent=0').fetchone()[0],
                    'vps_deployed':False,'live_tape_authenticated':False,'telegram_delivered':False}
            text=json.dumps(report,indent=2)
            print(text)
            if len(sys.argv)>1:Path(sys.argv[1]).write_text(text)
        finally:
            task.cancel()
            await asyncio.gather(task,return_exceptions=True)


if __name__=='__main__':asyncio.run(check())
