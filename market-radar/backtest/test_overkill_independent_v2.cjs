#!/usr/bin/env node
'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const enginePath = path.join(__dirname, 'overkill_independent_v2.js');
const runner = eval(fs.readFileSync(enginePath, 'utf8'));
const resultPath = path.join(__dirname, 'results/overkill-independent-v2-no-target-2026-10-26.json');
const frozen = JSON.parse(fs.readFileSync(resultPath, 'utf8'));
assert.equal(frozen.signal_rules.takeProfitPct, null);
assert.equal(frozen.stocks.cleaned, 206);
assert.equal(frozen.crypto.tested, 22);
assert.equal(frozen.stocks.tickers.length, 206);
assert.equal(new Set(frozen.stocks.tickers).size, 206);
assert.equal(frozen.crypto.tickers.length, 22);

function fakeDailyOHLC() {
  const data = [];
  for (let i = 0; i < 2122; i++) {
    const px = 100 + 6*Math.sin(i/13) + 3*Math.sin(i/29) + 0.02*i;
    data.push({t: Date.UTC(2021, 0, 1+i), o:px, h:px+2, l:px-2, c:px});
  }
  return data;
}
async function main(){
  const bars=fakeDailyOHLC();
  const base=await runner(['SYNTH'], async()=>bars, 'stocks');
  assert.equal(base.good.length, 1);
  assert.equal(base.good[0].oos.length, 3);
  assert.ok(base.good[0].oos[0].closed > 0);
  assert.ok(base.good[0].oos[0].ret > .10, 'Strategy return may pass 10%, not capped by any 10% target');
  for(const period of ['train','oos']){
    for(const x of base.good[0][period]){
      assert.ok(x && Number.isFinite(x.ret) && Number.isFinite(x.maxDD));
      assert.ok(x.closed>=0 && x.wins<=x.closed);
    }
  }
  // An artificial 5:1 split must not generate false entry/exit returns:
  const split=bars.map(b=>b.t >= Date.parse('2025-12-18')
    ? {...b, o:b.o/5, h:b.h/5, l:b.l/5, c:b.c/5} : {...b});
  const adjusted=await runner(['SYNTH_SPLIT'],async()=>split,'stocks');
  assert.equal(adjusted.good[0].actions.some(x=>x.split===5),true);
  for(let k=0;k<3;k++){
    assert.ok(Math.abs(adjusted.good[0].oos[k].ret-base.good[0].oos[k].ret)<1e-6,
      'Split-adjusted result should be invariant under artificial 5:1 split');
  }
  // Additional data after the OOS end must not influence the earlier OOS output.
  const future=bars.concat(Array.from({length:90},(_,i)=>{
    const t=Date.UTC(2026,9,24+i);
    const price=400+20*Math.sin(i);
    return {t,o:price,h:price+5,l:price-5,c:price};
  }));
  const noLeak=await runner(['SYNTH_FUTURE'],async()=>future,'stocks');
  for(let k=0;k<3;k++) {
    assert.ok(Math.abs(noLeak.good[0].oos[k].ret-base.good[0].oos[k].ret)<1e-6,
      'Out-of-sample result must not depend on future prices beyond the end date');
  }
  console.log('PASS: original v2 rules; 3 modes; no 10% TP; 5:1 split invariance; post-horizon no-peek; frozen sample counts');
}
main().catch(e=>{console.error(e);process.exitCode=1;});
